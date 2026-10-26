package mobile

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"net/http"
	"net/netip"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"tailscale.com/ipn"
	"tailscale.com/ipn/ipnstate"
	"tailscale.com/tailcfg"
	"tailscale.com/types/key"
)

func TestDiscoveryHasFourIndependentSlotsAndCoalescesScans(t *testing.T) {
	f := testNetwork()
	for i := 0; i < 8; i++ {
		f.status.Peer[key.NewNode().Public()] = &ipnstate.PeerStatus{ID: tailcfg.StableNodeID("n-extra-" + string(rune('a'+i))), UserID: 42, Online: true,
			TailscaleIPs: []netip.Addr{netip.MustParseAddr("100.100.1.3")}}
	}
	var active, maximum, calls atomic.Int32
	started, release := make(chan struct{}, 20), make(chan struct{})
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	dial := func(ctx context.Context, _, _ string) (net.Conn, error) {
		calls.Add(1)
		current := active.Add(1)
		defer active.Add(-1)
		for old := maximum.Load(); current > old; old = maximum.Load() {
			if maximum.CompareAndSwap(old, current) {
				break
			}
		}
		started <- struct{}{}
		select {
		case <-release:
		case <-ctx.Done():
		}
		return nil, errors.New("ordinary peer")
	}
	d := newHostDiscovery(nil)
	// Occupying every normal request/poll slot cannot block discovery.
	n := &node{requests: make(chan struct{}, 12), polls: make(chan struct{}, 64)}
	for range cap(n.requests) {
		n.requests <- struct{}{}
	}
	for range cap(n.polls) {
		n.polls <- struct{}{}
	}
	if !d.snapshot(ctx, f.status, f, dial, time.Now()).Discovering {
		t.Fatal("no active scan")
	}
	for range 4 {
		select {
		case <-started:
		case <-time.After(time.Second):
			t.Fatal("discovery did not run independently")
		}
	}
	for range 20 {
		start := time.Now()
		snapshot := d.snapshot(ctx, f.status, f, noDial(t), time.Now())
		if !snapshot.Discovering || len(snapshot.Peers) != 0 || time.Since(start) > 100*time.Millisecond {
			t.Fatal("snapshot waited for probes or exposed peers")
		}
	}
	if calls.Load() != 4 {
		t.Fatalf("coalescing/concurrency broken: calls=%d", calls.Load())
	}
	close(release)
	waitDiscovery(t, d)
	if maximum.Load() != 4 || calls.Load() != 9 {
		t.Fatalf("maximum=%d calls=%d", maximum.Load(), calls.Load())
	}
	if d.snapshot(ctx, f.status, f, noDial(t), time.Now()).Discovering {
		t.Fatal("negative cache did not settle")
	}
}

func TestConfirmedIDCacheIsPrivateOwnerBoundAndClearedOnLogout(t *testing.T) {
	directory := t.TempDir()
	store, err := openStateStore(directory)
	if err != nil {
		t.Fatal(err)
	}
	d, f := newHostDiscovery(store), testNetwork()
	dial := discoveryServer(t, func(w http.ResponseWriter, r *http.Request) {
		checkProbeRequest(t, r)
		_, _ = io.WriteString(w, discoverySignature)
	})
	d.snapshot(context.Background(), f.status, f, dial, time.Now())
	waitDiscovery(t, d)
	data, err := store.ReadState(discoveryStateKey)
	if err != nil {
		t.Fatal(err)
	}
	var saved confirmedHosts
	if json.Unmarshal(data, &saved) != nil || saved.Owner != "n-self/42" || len(saved.IDs) != 1 || saved.IDs[0] != "n-peer" || strings.Contains(string(data), "hostname") || strings.Contains(string(data), "100.100") {
		t.Fatalf("unexpected cache %s", data)
	}
	if runtime.GOOS != "windows" {
		info, err := os.Stat(filepath.Join(directory, "tailscaled.state"))
		if err != nil || info.Mode().Perm() != 0600 {
			t.Fatal("cache not private")
		}
	}
	// Restart reads only established IDs, even while a new probe is pending.
	store, err = openStateStore(directory)
	if err != nil {
		t.Fatal(err)
	}
	restarted := newHostDiscovery(store)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	blocked := func(ctx context.Context, _, _ string) (net.Conn, error) { <-ctx.Done(); return nil, ctx.Err() }
	start := time.Now()
	snapshot := restarted.snapshot(ctx, f.status, f, blocked, start)
	if len(snapshot.Peers) != 1 || !snapshot.Discovering || time.Since(start) > 100*time.Millisecond {
		t.Fatal("confirmed host waited for discovery")
	}
	cancel()
	waitDiscovery(t, restarted)
	// Owner changes never reuse the cache, including for offline peers.
	f.status.Self.UserID = 99
	for _, p := range f.status.Peer {
		p.UserID = 99
		p.Online = false
	}
	if snapshot = restarted.snapshot(context.Background(), f.status, f, noDial(t), time.Now()); len(snapshot.Peers) != 0 || snapshot.Discovering {
		t.Fatal("previous owner's host leaked")
	}
	f.status.Self.UserID = 42
	for _, p := range f.status.Peer {
		p.UserID = 42
	}
	if len(newHostDiscovery(store).snapshot(context.Background(), f.status, f, noDial(t), time.Now()).Peers) != 1 {
		t.Fatal("offline confirmed cache lost")
	}
	restarted.clear()
	if _, err := store.ReadState(discoveryStateKey); !errors.Is(err, ipn.ErrStateNotExist) {
		t.Fatal("logout retained confirmed cache")
	}
	if len(newHostDiscovery(store).snapshot(context.Background(), f.status, f, noDial(t), time.Now()).Peers) != 0 {
		t.Fatal("logout restored cache")
	}
}

func TestDiscoveryCacheRejectsCorruptionAndUnverifiedIDs(t *testing.T) {
	store, err := openStateStore(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	for _, raw := range []string{
		"{", "null", `{"owner":"n-self/42","ids":["n-peer",""]}`, `{"owner":"n-self/99","ids":["n-peer"]}`,
		`{"owner":"n-self/42","ids":["n-peer",` + `"` + strings.Repeat("n", 257) + `"]}`,
		strings.Repeat("x", 300*1024+1),
	} {
		if err := store.WriteState(discoveryStateKey, []byte(raw)); err != nil {
			t.Fatal(err)
		}
		f := testNetwork()
		for _, p := range f.status.Peer {
			p.Online = false
		}
		if snapshot := newHostDiscovery(store).snapshot(context.Background(), f.status, f, noDial(t), time.Now()); len(snapshot.Peers) != 0 || snapshot.Discovering {
			t.Fatalf("corrupt cache accepted %q", raw[:min(len(raw), 100)])
		}
	}
	// A remembered or Shellfox-named ordinary node is never persisted.
	d, f := newHostDiscovery(store), testNetwork()
	for _, p := range f.status.Peer {
		p.HostName = "shellfox-remembered"
	}
	failed := func(context.Context, string, string) (net.Conn, error) { return nil, errors.New("ordinary") }
	d.snapshot(context.Background(), f.status, f, failed, time.Now())
	waitDiscovery(t, d)
	d.mu.Lock()
	d.saveLocked()
	d.mu.Unlock()
	data, _ := store.ReadState(discoveryStateKey)
	if strings.Contains(string(data), "n-peer") {
		t.Fatal("unverified peer persisted")
	}
}

type failedDiscoveryStore struct{}

func (failedDiscoveryStore) ReadState(ipn.StateKey) ([]byte, error) { return nil, ipn.ErrStateNotExist }
func (failedDiscoveryStore) WriteState(ipn.StateKey, []byte) error {
	return errors.New("private filesystem diagnostic")
}

func TestDiscoveryClearReportsPersistenceFailureWithoutLeakingDiagnostics(t *testing.T) {
	d := newHostDiscovery(failedDiscoveryStore{})
	d.owner = "n-self/42"
	d.entries["n-peer"] = hostEntry{confirmed: true}
	if err := d.clear(); err == nil || err.Error() != "Cannot clear network hosts" {
		t.Fatalf("unexpected clear result %v", err)
	}
	if len(d.entries) != 0 || d.owner != "" {
		t.Fatal("failed persistence kept in-memory hosts")
	}
}

func TestDiscoveryDropsRevokedOwnershipAndCancelsOnLogout(t *testing.T) {
	d, f := newHostDiscovery(nil), testNetwork()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	started := make(chan struct{}, 1)
	blocked := func(ctx context.Context, _, _ string) (net.Conn, error) {
		started <- struct{}{}
		<-ctx.Done()
		return nil, ctx.Err()
	}
	d.snapshot(ctx, f.status, f, blocked, time.Now())
	<-started
	// A cancelled scan cannot repopulate state after clear.
	d.clear()
	waitDiscovery(t, d)
	if len(d.entries) != 0 {
		t.Fatal("cleared discovery restored entries")
	}
	f.status.Self.UserID = 99
	if snapshot := d.snapshot(ctx, f.status, f, noDial(t), time.Now()); snapshot.Discovering || len(snapshot.Peers) != 0 {
		t.Fatal("foreign peer discovered")
	}
}
