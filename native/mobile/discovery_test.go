package mobile

import (
	"context"
	"errors"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func discoveryServer(t *testing.T, handler http.HandlerFunc) dialFunc {
	t.Helper()
	server := httptest.NewServer(handler)
	t.Cleanup(server.Close)
	return func(ctx context.Context, network, address string) (net.Conn, error) {
		if network != "tcp" || address != "100.100.1.2:52743" {
			t.Errorf("unsafe target %s %s", network, address)
		}
		return (&net.Dialer{}).DialContext(ctx, "tcp", server.Listener.Addr().String())
	}
}

func checkProbeRequest(t *testing.T, r *http.Request) {
	t.Helper()
	body, _ := io.ReadAll(r.Body)
	if r.Method != "POST" || string(body) != "{}" || r.Header.Get("Content-Type") != "application/json" {
		t.Errorf("discovery must be an identity-free JSON POST: %s %s", r.Method, body)
	}
	if r.Header.Get("Authorization") != "" || r.Header.Get("X-Shellfox-Peer") != "" {
		t.Error("unexpected caller headers")
	}
}

func TestDiscoveryRequiresExactServiceSignature(t *testing.T) {
	cases := []struct {
		name, body string
		status     int
		want       bool
	}{
		{"current", discoverySignature, 200, true},
		{"reordered", `{"value":{"protocolVersion":1,"service":"shellfox"},"ok":true}`, 200, true},
		{"legacy", legacySignature, 404, true},
		{"ordinary", `{"ok":true}`, 200, false},
		{"wrong service", `{"ok":true,"value":{"service":"ssh","protocolVersion":1}}`, 200, false},
		{"wrong version", `{"ok":true,"value":{"service":"shellfox","protocolVersion":2}}`, 200, false},
		{"extra metadata", `{"ok":true,"value":{"service":"shellfox","protocolVersion":1,"hostname":"shellfox"}}`, 200, false},
		{"not JSON", "Shellfox", 200, false},
		{"oversized", strings.Repeat("x", 4097), 200, false},
		{"unauthenticated", discoverySignature, 403, false},
		{"rate limited current", `{"ok":false,"error":{"code":"VALIDATION","message":"Remote request rate exceeded.","retryable":true}}`, 200, false},
		{"rate limited legacy", `{"ok":false,"error":{"code":"VALIDATION","message":"Remote request rate exceeded.","retryable":true}}`, 404, false},
		{"redirect", discoverySignature, 307, false},
		{"legacy generic validation", `{"ok":false,"error":{"code":"VALIDATION","message":"Invalid request","retryable":true}}`, 404, false},
		{"legacy approval", `{"ok":false,"error":{"code":"APPROVAL_REQUIRED","message":"Approve this device in Shellfox on the host first.","retryable":true}}`, 404, false},
		{"legacy wrong retry", `{"ok":false,"error":{"code":"VALIDATION","message":"Remote request could not be completed.","retryable":false}}`, 404, false},
		{"legacy missing retry", `{"ok":false,"error":{"code":"VALIDATION","message":"Remote request could not be completed."}}`, 404, false},
		{"legacy extra", `{"ok":false,"error":{"code":"VALIDATION","message":"Remote request could not be completed.","retryable":true},"service":"shellfox"}`, 404, false},
		{"legacy unauthorized", `{"ok":false,"error":{"code":"AUTH_FAILED","message":"Unverified remote peer.","retryable":false}}`, 404, false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			var calls atomic.Int32
			dial := discoveryServer(t, func(w http.ResponseWriter, r *http.Request) {
				checkProbeRequest(t, r)
				call := calls.Add(1)
				if call == 1 {
					if r.URL.Path != "/v1/discover" {
						t.Error("discovery not first")
					}
					w.WriteHeader(tc.status)
					if tc.status == 404 {
						_, _ = io.WriteString(w, "404 page not found")
						return
					}
				} else if r.URL.Path != "/v1/request" || tc.status != 404 {
					t.Error("unsafe fallback")
				}
				_, _ = io.WriteString(w, tc.body)
			})
			if got := probeHost(context.Background(), testNetwork(), dial, "n-peer"); got != tc.want {
				t.Fatalf("confirmed=%v want %v", got, tc.want)
			}
			wantCalls := int32(1)
			if tc.status == 404 {
				wantCalls = 2
			}
			if calls.Load() != wantCalls {
				t.Fatalf("calls=%d want %d", calls.Load(), wantCalls)
			}
		})
	}
}

func TestDiscoveryAndLegacyShareTwoSecondDeadline(t *testing.T) {
	var calls atomic.Int32
	dial := discoveryServer(t, func(w http.ResponseWriter, r *http.Request) {
		checkProbeRequest(t, r)
		if calls.Add(1) == 1 {
			select {
			case <-time.After(1100 * time.Millisecond):
				w.WriteHeader(404)
			case <-r.Context().Done():
			}
		} else {
			<-r.Context().Done()
		}
	})
	start := time.Now()
	if probeHost(context.Background(), testNetwork(), dial, "n-peer") {
		t.Fatal("unresponsive legacy accepted")
	}
	if elapsed := time.Since(start); elapsed > 2700*time.Millisecond || elapsed < 1800*time.Millisecond {
		t.Fatalf("unbounded or missing shared deadline: %v", elapsed)
	}
	if calls.Load() != 2 {
		t.Fatal("did not exercise legacy deadline")
	}
}

func waitDiscovery(t *testing.T, d *hostDiscovery) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		d.mu.Lock()
		pending := d.pending
		d.mu.Unlock()
		if pending == 0 {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("discovery never settled")
}

func TestDiscoveryPositiveNegativeCachesAndOfflineRetention(t *testing.T) {
	for _, confirmed := range []bool{false, true} {
		t.Run(map[bool]string{false: "ordinary", true: "host"}[confirmed], func(t *testing.T) {
			var calls atomic.Int32
			dial := discoveryServer(t, func(w http.ResponseWriter, r *http.Request) {
				checkProbeRequest(t, r)
				calls.Add(1)
				body := `{"ok":true}`
				if confirmed {
					body = discoverySignature
				}
				_, _ = io.WriteString(w, body)
			})
			d, f, now := newHostDiscovery(nil), testNetwork(), time.Now()
			// Hostnames are deliberately misleading in both directions.
			for _, p := range f.status.Peer {
				if confirmed {
					p.HostName = "plain-laptop"
				} else {
					p.HostName = "shellfox-server"
				}
			}
			first := d.snapshot(context.Background(), f.status, f, dial, now)
			if !first.Discovering || len(first.Peers) != 0 {
				t.Fatal("initial scan must be in progress, not an unverified host")
			}
			waitDiscovery(t, d)
			for range 10 {
				snapshot := d.snapshot(context.Background(), f.status, f, noDial(t), now.Add(3*time.Second))
				if snapshot.Discovering || (len(snapshot.Peers) == 1) != confirmed {
					t.Fatalf("bad cached snapshot: %+v", snapshot)
				}
			}
			if calls.Load() != 1 {
				t.Fatal("cache hammered peer")
			}
			if confirmed {
				for _, p := range f.status.Peer {
					p.Online = false
				}
				snapshot := d.snapshot(context.Background(), f.status, f, noDial(t), now.Add(6*time.Minute))
				if snapshot.Discovering || len(snapshot.Peers) != 1 || snapshot.Peers[0].Online {
					t.Fatal("offline verified host lost")
				}
				for _, p := range f.status.Peer {
					p.Online = true
				}
			}
			failedDial := func(context.Context, string, string) (net.Conn, error) { return nil, errors.New("unreachable") }
			if !d.snapshot(context.Background(), f.status, f, failedDial, now.Add(6*time.Minute)).Discovering {
				t.Fatal("expired entry not reprobed")
			}
			waitDiscovery(t, d)
			snapshot := d.snapshot(context.Background(), f.status, f, noDial(t), time.Now())
			if (len(snapshot.Peers) == 1) != confirmed {
				t.Fatal("transient failure changed established identity")
			}
		})
	}
}
