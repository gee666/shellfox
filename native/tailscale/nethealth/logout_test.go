package nethealth

import (
	"context"
	"sync/atomic"
	"testing"
	"time"

	"tailscale.com/health"
	"tailscale.com/tsconst"
)

func TestExplicitLogoutWaitsForBlockedRecoveryAndPreventsFurtherRestarts(t *testing.T) {
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
	logout := make(chan struct{})
	var raced atomic.Bool
	go func() {
		_ = m.Exclusive(func() error {
			f.mu.Lock()
			defer f.mu.Unlock()
			if f.active != 0 {
				raced.Store(true)
			}
			f.stopped = true
			f.online = false
			return nil
		})
		close(logout)
	}()
	select {
	case <-logout:
		t.Fatal("logout overlapped an in-flight control restart")
	case <-time.After(10 * time.Millisecond):
	}
	close(f.block)
	<-logout
	for i := 0; i < 100; i++ {
		m.Wake()
	}
	time.Sleep(100 * time.Millisecond)
	if starts, _, peak := f.counts(); raced.Load() || starts != 1 || peak != 1 {
		t.Fatal("logout allowed a concurrent or subsequent recovery")
	}
	cancel()
	<-done
}
func TestDegradedTransportKeepsLiveControlAndOnlyRepairsBindings(t *testing.T) {
	f := &fakeClient{online: true}
	var subscriptions atomic.Int32
	s := source(f, &subscriptions)
	s.Health = func() *health.State { return warning(tsconst.HealthWarnableNoUDP4Bind, true) }
	m, err := New(s, fastConfig(), nil)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() { defer close(done); m.Run(ctx) }()
	wait(t, func() bool { _, rebinds, _ := f.counts(); return rebinds >= 2 })
	status := m.Status(ctx)
	if starts, _, _ := f.counts(); starts != 0 || status.Status != "online" || status.Error == nil {
		t.Fatal("a relay/socket warning destroyed healthy control or hid degraded health")
	}
	cancel()
	<-done
}
