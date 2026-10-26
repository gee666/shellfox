package mobile

import (
	"context"
	"errors"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"strings"
	"testing"
	"time"

	"tailscale.com/ipn/ipnstate"
	"tailscale.com/types/key"
	"tailscale.com/types/views"
)

type fakeNetwork struct {
	status *ipnstate.Status
	err    error
}

func (f *fakeNetwork) Status(context.Context) (*ipnstate.Status, error) { return f.status, f.err }
func testNetwork() *fakeNetwork {
	p := &ipnstate.PeerStatus{ID: "n-peer", HostName: "laptop", UserID: 42, Online: true,
		TailscaleIPs: []netip.Addr{netip.MustParseAddr("100.100.1.2"), netip.MustParseAddr("fd7a:115c:a1e0::2")}}
	return &fakeNetwork{status: &ipnstate.Status{BackendState: "Running", Self: &ipnstate.PeerStatus{ID: "n-self", UserID: 42}, Peer: map[key.NodePublic]*ipnstate.PeerStatus{{}: p}}}
}

// Exercise one request in isolation, without retaining a pool across test cases.
func request(ctx context.Context, client peerClient, dial dialFunc, peerID, path, body string) (string, error) {
	pool := &transportPool{}
	defer pool.close()
	return pool.request(ctx, client, dial, peerID, path, body)
}

func noDial(t *testing.T) dialFunc {
	t.Helper()
	return func(context.Context, string, string) (net.Conn, error) {
		t.Error("unexpected dial")
		return nil, errors.New("blocked")
	}
}

func TestRejectsUnsafeRequestsBeforeDial(t *testing.T) {
	cases := [][3]string{
		{"", "/v1/request", "{}"}, {strings.Repeat("n", 257), "/v1/request", "{}"},
		{"n-peer", "http://localhost/", "{}"}, {"n-peer", "/v1/request?secret=key", "{}"},
		{"n-peer", "/v1/../request", "{}"}, {"n-peer", "/v1/%72equest", "{}"},
		{"n-peer", "/v1/events#fragment", "{}"}, {"n-peer", "/v1/request", "{"},
		{"n-peer", "/v1/request", ""}, {"n-peer", "/v1/request", `"` + strings.Repeat("x", bodyLimit) + `"`},
	}
	for _, c := range cases {
		if _, err := request(context.Background(), testNetwork(), noDial(t), c[0], c[1], c[2]); err == nil {
			t.Fatalf("accepted %q", c[1])
		}
	}
}

func TestOwnershipAndReachabilityFailClosed(t *testing.T) {
	cases := map[string]func(*ipnstate.Status, *ipnstate.PeerStatus){
		"foreign":    func(s *ipnstate.Status, p *ipnstate.PeerStatus) { p.UserID = 7 },
		"logged out": func(s *ipnstate.Status, p *ipnstate.PeerStatus) { s.BackendState = "NeedsLogin" },
		"no self":    func(s *ipnstate.Status, p *ipnstate.PeerStatus) { s.Self = nil },
		"zero owner": func(s *ipnstate.Status, p *ipnstate.PeerStatus) { s.Self.UserID = 0 },
		"self":       func(s *ipnstate.Status, p *ipnstate.PeerStatus) { p.ID = s.Self.ID },
		"tagged": func(s *ipnstate.Status, p *ipnstate.PeerStatus) {
			tags := views.SliceOf([]string{"tag:server"})
			p.Tags = &tags
		},
		"offline": func(s *ipnstate.Status, p *ipnstate.PeerStatus) { p.Online = false },
		"loopback": func(s *ipnstate.Status, p *ipnstate.PeerStatus) {
			p.TailscaleIPs = []netip.Addr{netip.MustParseAddr("127.0.0.1")}
		},
		"mapped": func(s *ipnstate.Status, p *ipnstate.PeerStatus) {
			p.TailscaleIPs = []netip.Addr{netip.MustParseAddr("::ffff:100.100.1.2")}
		},
	}
	for name, mutate := range cases {
		t.Run(name, func(t *testing.T) {
			f := testNetwork()
			for _, p := range f.status.Peer {
				mutate(f.status, p)
			}
			if _, err := request(context.Background(), f, noDial(t), "n-peer", "/v1/request", "{}"); err == nil {
				t.Fatal("unauthorized peer accepted")
			}
		})
	}
	if peers := ownPeers(nil); peers == nil || len(peers) != 0 {
		t.Fatal("nil status must give empty array")
	}
	if _, err := request(context.Background(), testNetwork(), noDial(t), "127.0.0.1", "/v1/request", "{}"); err == nil {
		t.Fatal("IP accepted as ID")
	}
}

func TestForwardingUsesOnlyFixedTailnetPortAndJSON(t *testing.T) {
	for _, path := range []string{"/v1/discover", "/v1/pair", "/v1/request", "/v1/events", "/v1/disconnect"} {
		t.Run(path, func(t *testing.T) {
			upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if r.Method != "POST" || r.URL.Path != path || r.Header.Get("Content-Type") != "application/json" {
					t.Error("wrong request")
				}
				if r.Header.Get("Authorization") != "" || r.Header.Get("X-Shellfox-Peer") != "" {
					t.Error("caller identity leaked")
				}
				data, _ := io.ReadAll(r.Body)
				if string(data) != `{"clientId":"test"}` {
					t.Error("body changed")
				}
				_, _ = w.Write([]byte(`{"ok":true,"value":{}}`))
			}))
			defer upstream.Close()
			dials := 0
			dial := func(ctx context.Context, network, address string) (net.Conn, error) {
				dials++
				if network != "tcp" || address != "100.100.1.2:52743" {
					t.Fatalf("unsafe dial %s %s", network, address)
				}
				return (&net.Dialer{}).DialContext(ctx, "tcp", upstream.Listener.Addr().String())
			}
			f := testNetwork()
			data, err := request(context.Background(), f, dial, "n-peer", path, `{"clientId":"test"}`)
			if err != nil || data != `{"ok":true,"value":{}}` || dials != 1 {
				t.Fatalf("%q %v dials=%d", data, err, dials)
			}
			// A connection from a previous authorized map must not bypass revocation.
			f.status.Self.UserID = 9
			if _, err := request(context.Background(), f, noDial(t), "n-peer", path, "{}"); err == nil {
				t.Fatal("stale ownership accepted")
			}
		})
	}
}

func TestResponseBoundsAndRedirects(t *testing.T) {
	for _, mode := range []string{"redirect", "too large", "not json", "error", "empty", "compressed"} {
		t.Run(mode, func(t *testing.T) {
			calls := 0
			upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				calls++
				switch mode {
				case "redirect":
					w.Header().Set("Location", "http://127.0.0.1/steal")
					w.WriteHeader(307)
				case "too large":
					_, _ = io.WriteString(w, `"`+strings.Repeat("x", responseLimit)+`"`)
				case "not json":
					_, _ = io.WriteString(w, "sdk secret diagnostics")
				case "error":
					w.WriteHeader(403)
					_, _ = io.WriteString(w, "sdk secret diagnostics")
				case "compressed":
					w.Header().Set("Content-Encoding", "gzip")
					_, _ = w.Write([]byte{0x1f, 0x8b})
				}
			}))
			defer upstream.Close()
			dial := func(ctx context.Context, _, _ string) (net.Conn, error) {
				return (&net.Dialer{}).DialContext(ctx, "tcp", upstream.Listener.Addr().String())
			}
			data, err := request(context.Background(), testNetwork(), dial, "n-peer", "/v1/request", "{}")
			if err == nil || data != "" || strings.Contains(err.Error(), "secret") || calls != 1 {
				t.Fatalf("unsafe response %q %v calls=%d", data, err, calls)
			}
		})
	}
}

func TestCancelInterruptsLongPoll(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	started := make(chan struct{})
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = io.Copy(io.Discard, r.Body)
		close(started)
		<-r.Context().Done()
	}))
	defer upstream.Close()
	dial := func(ctx context.Context, _, _ string) (net.Conn, error) {
		return (&net.Dialer{}).DialContext(ctx, "tcp", upstream.Listener.Addr().String())
	}
	done := make(chan error, 1)
	go func() { _, err := request(ctx, testNetwork(), dial, "n-peer", "/v1/events", "{}"); done <- err }()
	select {
	case <-started:
	case <-time.After(5 * time.Second):
		t.Fatal("poll never started")
	}
	cancel()
	select {
	case err := <-done:
		if err == nil {
			t.Fatal("canceled poll succeeded")
		}
	case <-time.After(5 * time.Second):
		t.Fatal("canceled poll stuck")
	}
}
