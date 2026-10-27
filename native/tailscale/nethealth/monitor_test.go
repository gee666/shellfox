package nethealth

import (
	"context"
	"errors"
	"reflect"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"tailscale.com/health"
	"tailscale.com/ipn"
	"tailscale.com/ipn/ipnstate"
)

type fakeClient struct {
	mu                            sync.Mutex
	online                        bool
	stopped                       bool
	failure                       bool
	starts, rebinds, active, peak int
	opts                          []ipn.Options
	block                         chan struct{}
}

func (f *fakeClient) StatusWithoutPeers(context.Context) (*ipnstate.Status, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	st := connected()
	st.Self.Online = f.online
	if f.stopped {
		st.BackendState = "Stopped"
	}
	return st, nil
}
func (f *fakeClient) DebugAction(_ context.Context, action string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if action != "rebind" {
		return errors.New("unexpected debug action")
	}
	f.rebinds++
	return nil
}
func (f *fakeClient) Start(ctx context.Context, opts ipn.Options) error {
	f.mu.Lock()
	f.starts++
	f.active++
	f.peak = max(f.peak, f.active)
	f.opts = append(f.opts, opts)
	block := f.block
	f.mu.Unlock()
	defer func() { f.mu.Lock(); f.active--; f.mu.Unlock() }()
	if block != nil {
		select {
		case <-block:
		case <-ctx.Done():
			return ctx.Err()
		}
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.failure {
		return errors.New("private upstream error")
	}
	f.online = true
	return nil
}
func (f *fakeClient) counts() (int, int, int) {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.starts, f.rebinds, f.peak
}
func wait(t *testing.T, check func() bool) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for !check() {
		if time.Now().After(deadline) {
			t.Fatal("timed out waiting for recovery")
		}
		time.Sleep(time.Millisecond)
	}
}
func source(f *fakeClient, subscribes *atomic.Int32) Source {
	return Source{Client: f, Health: func() *health.State { return nil }, NetworkState: func() LinkState { return LinkUp }, RefreshNetwork: func() {},
		Subscribe: func(func()) func() { subscribes.Add(1); return func() { subscribes.Add(-1) } },
	}
}
func fastConfig() Config {
	return Config{CheckInterval: time.Millisecond, Grace: 20 * time.Millisecond, MaxBackoff: 80 * time.Millisecond, CallTimeout: time.Second}
}

func TestSingleWorkerDrainsLinkFlapsAndKeepsIdentityOptions(t *testing.T) {
	f := &fakeClient{block: make(chan struct{})}
	var subscriptions atomic.Int32
	m, err := New(source(f, &subscriptions), fastConfig(), nil)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan struct{})
	go func() { defer close(done); m.Run(ctx) }()
	wait(t, func() bool { starts, _, _ := f.counts(); return starts == 1 })
	extra := make(chan struct{})
	go func() { defer close(extra); m.Run(ctx) }() // Accidental double-start must not create another worker.
	select {
	case <-extra:
	case <-time.After(time.Second):
		t.Fatal("a second recovery worker started")
	}
	for i := 0; i < 1000; i++ {
		m.Wake()
	}
	time.Sleep(10 * time.Millisecond)
	starts, rebinds, peak := f.counts()
	if starts != 1 || rebinds != 1 || peak != 1 || subscriptions.Load() != 1 {
		t.Fatalf("overlapping/churning recovery: starts=%d rebinds=%d peak=%d", starts, rebinds, peak)
	}
	close(f.block)
	wait(t, func() bool { return m.Status(ctx).Status == "online" })
	time.Sleep(100 * time.Millisecond)
	if starts, _, _ = f.counts(); starts != 1 {
		t.Fatal("healthy control was repeatedly restarted")
	}
	f.mu.Lock()
	opts := append([]ipn.Options(nil), f.opts...)
	f.mu.Unlock()
	for _, option := range opts {
		if !reflect.DeepEqual(option, ipn.Options{}) {
			t.Fatal("recovery changed auth keys, identity or preferences")
		}
	}
	cancel()
	<-done
	if subscriptions.Load() != 0 {
		t.Fatal("network callback retained after shutdown")
	}
}
func TestFailedRecoveryNeverPublishesOnline(t *testing.T) {
	f := &fakeClient{failure: true}
	var subscriptions atomic.Int32
	var falseOnline atomic.Bool
	m, err := New(source(f, &subscriptions), fastConfig(), func(status Status) {
		if status.Status == "online" {
			falseOnline.Store(true)
		}
	})
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() { defer close(done); m.Run(ctx) }()
	wait(t, func() bool { starts, _, _ := f.counts(); return starts >= 2 })
	if m.Status(ctx).Status != "error" || falseOnline.Load() {
		t.Fatal("failed recovery reported online")
	}
	cancel()
	<-done
}
func TestExplicitEnrollmentExcludesRecoveryAndRechecksFreshHealth(t *testing.T) {
	f := &fakeClient{}
	var subscriptions atomic.Int32
	m, err := New(source(f, &subscriptions), fastConfig(), nil)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan struct{})
	go func() { defer close(done); m.Run(ctx) }()
	if err := m.Exclusive(func() error {
		time.Sleep(70 * time.Millisecond)
		f.mu.Lock()
		f.online = true
		f.mu.Unlock()
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	time.Sleep(70 * time.Millisecond)
	if starts, rebinds, _ := f.counts(); starts != 0 || rebinds != 0 {
		t.Fatal("recovery raced explicit enrollment")
	}
	cancel()
	<-done
}
