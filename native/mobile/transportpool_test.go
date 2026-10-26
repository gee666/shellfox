package mobile

import (
	"context"
	"errors"
	"net"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"sync/atomic"
	"testing"
	"time"

	"tailscale.com/types/key"
	"tailscale.com/types/views"
)

func poolServer(t *testing.T, handler http.HandlerFunc) (*transportPool, *fakeNetwork, dialFunc, *atomic.Int32) {
	t.Helper()
	server := httptest.NewServer(handler)
	t.Cleanup(server.Close)
	pool := &transportPool{}
	t.Cleanup(pool.close)
	dials := &atomic.Int32{}
	dial := func(ctx context.Context, network, target string) (net.Conn, error) {
		if network != "tcp" || target == "" {
			return nil, errors.New("unexpected target")
		}
		dials.Add(1)
		return (&net.Dialer{}).DialContext(ctx, network, server.Listener.Addr().String())
	}
	return pool, testNetwork(), dial, dials
}
func jsonReply(w http.ResponseWriter, _ *http.Request) { _, _ = w.Write([]byte(`{"ok":true}`)) }

func TestPoolReusesOnlyUnchangedIdentities(t *testing.T) {
	mutations := map[string]func(*fakeNetwork){
		"self ID":  func(f *fakeNetwork) { f.status.Self.ID = "n-new-self" },
		"self key": func(f *fakeNetwork) { f.status.Self.PublicKey = key.NewNode().Public() },
		"self IP": func(f *fakeNetwork) {
			f.status.Self.TailscaleIPs = []netip.Addr{netip.MustParseAddr("100.64.0.5")}
		},
		"owner": func(f *fakeNetwork) {
			f.status.Self.UserID = 99
			for _, peer := range f.status.Peer {
				peer.UserID = 99
			}
		},
		"peer key": func(f *fakeNetwork) {
			for _, peer := range f.status.Peer {
				peer.PublicKey = key.NewNode().Public()
			}
		},
		"map key": func(f *fakeNetwork) {
			for old, peer := range f.status.Peer {
				delete(f.status.Peer, old)
				f.status.Peer[key.NewNode().Public()] = peer
				break
			}
		},
		"address": func(f *fakeNetwork) {
			for _, peer := range f.status.Peer {
				peer.TailscaleIPs = []netip.Addr{netip.MustParseAddr("100.64.0.4")}
			}
		},
	}
	for name, mutate := range mutations {
		t.Run(name, func(t *testing.T) {
			pool, network, dial, dials := poolServer(t, jsonReply)
			send := func() {
				t.Helper()
				if _, err := pool.request(context.Background(), network, dial, "n-peer", "/v1/request", "{}"); err != nil {
					t.Fatal(err)
				}
			}
			send()
			send()
			if dials.Load() != 1 {
				t.Fatalf("unchanged identity dialed %d times", dials.Load())
			}
			mutate(network)
			send()
			if dials.Load() != 2 {
				t.Fatalf("changed identity reused connection: %d dials", dials.Load())
			}
		})
	}
}

func TestPoolRevocationFailureAndStopDiscardConnections(t *testing.T) {
	pool, network, dial, dials := poolServer(t, jsonReply)
	send := func() error {
		_, err := pool.request(context.Background(), network, dial, "n-peer", "/v1/request", "{}")
		return err
	}
	if err := send(); err != nil {
		t.Fatal(err)
	}
	network.status.Self.UserID = 7
	if send() == nil || dials.Load() != 1 {
		t.Fatal("revocation did not fail closed")
	}
	network.status.Self.UserID = 42
	if err := send(); err != nil {
		t.Fatal(err)
	}
	if dials.Load() != 2 {
		t.Fatal("reauthorized view reused revoked connection")
	}
	network.err = errors.New("status unavailable")
	if send() == nil {
		t.Fatal("accepted unavailable ownership map")
	}
	network.err = nil
	if err := send(); err != nil {
		t.Fatal(err)
	}
	if dials.Load() != 3 {
		t.Fatal("status failure kept idle connection")
	}
	pool.close()
	if send() == nil || dials.Load() != 3 {
		t.Fatal("stopped pool dialed")
	}
}

func TestPoolRechecksPolicyBeforeUsingWarmConnections(t *testing.T) {
	mutations := map[string]func(*fakeNetwork){
		"offline": func(f *fakeNetwork) {
			for _, peer := range f.status.Peer {
				peer.Online = false
			}
		},
		"foreign owner": func(f *fakeNetwork) {
			for _, peer := range f.status.Peer {
				peer.UserID = 8
			}
		},
		"tagged": func(f *fakeNetwork) {
			tags := views.SliceOf([]string{"tag:server"})
			for _, peer := range f.status.Peer {
				peer.Tags = &tags
			}
		},
		"invalid address": func(f *fakeNetwork) {
			for _, peer := range f.status.Peer {
				peer.TailscaleIPs = []netip.Addr{netip.MustParseAddr("127.0.0.1")}
			}
		},
		"missing peer": func(f *fakeNetwork) { f.status.Peer = nil },
		"no self":      func(f *fakeNetwork) { f.status.Self = nil },
		"zero owner":   func(f *fakeNetwork) { f.status.Self.UserID = 0 },
		"logged out":   func(f *fakeNetwork) { f.status.BackendState = "NeedsLogin" },
		"nil map":      func(f *fakeNetwork) { f.status = nil },
	}
	for name, mutate := range mutations {
		t.Run(name, func(t *testing.T) {
			calls := &atomic.Int32{}
			pool, network, dial, dials := poolServer(t, func(w http.ResponseWriter, r *http.Request) { calls.Add(1); jsonReply(w, r) })
			if _, err := pool.request(context.Background(), network, dial, "n-peer", "/v1/request", "{}"); err != nil {
				t.Fatal(err)
			}
			entry := pool.peers["n-peer"]
			if entry.http.DisableKeepAlives || entry.http.IdleConnTimeout >= 30*time.Second {
				t.Fatal("idle pool does not precede helper keep-alive deadline")
			}
			mutate(network)
			if _, err := pool.request(context.Background(), network, dial, "n-peer", "/v1/request", "{}"); err == nil {
				t.Fatal("warm connection bypassed policy")
			}
			if calls.Load() != 1 || dials.Load() != 1 || len(pool.peers) != 0 {
				t.Fatal("unauthorized request retained or used warm connection")
			}
		})
	}
}

func TestPoolLongPollDoesNotBlockInput(t *testing.T) {
	started, finish := make(chan struct{}), make(chan struct{})
	pool, network, dial, dials := poolServer(t, func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/v1/events" {
			close(started)
			<-finish
		}
		jsonReply(w, r)
	})
	defer close(finish)
	poll := make(chan error, 1)
	pollCtx, cancelPoll := context.WithCancel(context.Background())
	defer cancelPoll()
	go func() { _, err := pool.request(pollCtx, network, dial, "n-peer", "/v1/events", "{}"); poll <- err }()
	select {
	case <-started:
	case <-time.After(2 * time.Second):
		t.Fatal("poll not started")
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if _, err := pool.request(ctx, network, dial, "n-peer", "/v1/request", "{}"); err != nil {
		t.Fatal(err)
	}
	if dials.Load() != 2 {
		t.Fatalf("expected independent connections: %d", dials.Load())
	}
	cancelPoll()
	select {
	case err := <-poll:
		if err == nil {
			t.Fatal("cancelled poll succeeded")
		}
	case <-time.After(time.Second):
		t.Fatal("cancelled poll stuck")
	}
}

type failWriteConn struct {
	net.Conn
	fail *atomic.Bool
}

func (c *failWriteConn) Write(b []byte) (int, error) {
	if c.fail.Load() {
		return 0, errors.New("injected stale connection write error")
	}
	return c.Conn.Write(b)
}
func TestPoolDoesNotReplayPOSTOnStaleConnection(t *testing.T) {
	calls := &atomic.Int32{}
	pool, network, dial, dials := poolServer(t, func(w http.ResponseWriter, r *http.Request) { calls.Add(1); jsonReply(w, r) })
	fail := &atomic.Bool{}
	wrapped := func(ctx context.Context, network, address string) (net.Conn, error) {
		conn, err := dial(ctx, network, address)
		if err != nil {
			return nil, err
		}
		return &failWriteConn{conn, fail}, nil
	}
	if _, err := pool.request(context.Background(), network, wrapped, "n-peer", "/v1/request", "{}"); err != nil {
		t.Fatal(err)
	}
	fail.Store(true)
	if _, err := pool.request(context.Background(), network, wrapped, "n-peer", "/v1/request", "{}"); err == nil {
		t.Fatal("failed POST succeeded")
	}
	if dials.Load() != 1 || calls.Load() != 1 {
		t.Fatalf("POST replayed: dials=%d calls=%d", dials.Load(), calls.Load())
	}
}

func TestPoolAvoidsRepeatedDialLatency(t *testing.T) {
	for _, reuse := range []bool{false, true} {
		pool, network, dial, dials := poolServer(t, jsonReply)
		delayed := func(ctx context.Context, network, target string) (net.Conn, error) {
			time.Sleep(40 * time.Millisecond)
			return dial(ctx, network, target)
		}
		start := time.Now()
		for i := 0; i < 3; i++ {
			var err error
			if reuse {
				_, err = pool.request(context.Background(), network, delayed, "n-peer", "/v1/request", "{}")
			} else {
				_, err = request(context.Background(), network, delayed, "n-peer", "/v1/request", "{}")
			}
			if err != nil {
				t.Fatal(err)
			}
		}
		want := int32(3)
		if reuse {
			want = 1
		}
		if dials.Load() != want {
			t.Fatalf("reuse=%v dials=%d", reuse, dials.Load())
		}
		t.Logf("reuse=%v: 3 ordered requests, %d dials, %s", reuse, dials.Load(), time.Since(start))
	}
}
