package nethealth

import (
	"testing"
	"time"
)

func TestRecoveryGraceBackoffAndHealthyReset(t *testing.T) {
	p := policy{config: DefaultConfig()}
	now := time.Unix(100, 0)
	bad := Assessment{Status: Status{Status: "error"}, Eligible: true, NetworkUp: true, ControlLost: true}
	if p.decide(now, bad) != none || p.decide(now.Add(29*time.Second), bad) != none {
		t.Fatal("recovery skipped its grace period")
	}
	if p.decide(now.Add(30*time.Second), bad) != rebind {
		t.Fatal("first action must only refresh bindings")
	}
	for i := 0; i < 1000; i++ {
		if p.decide(now.Add(31*time.Second), bad) != none {
			t.Fatal("link chatter bypassed backoff")
		}
	}
	if p.decide(now.Add(60*time.Second), bad) != restartControl {
		t.Fatal("persistent control loss did not escalate")
	}
	if p.decide(now.Add(119*time.Second), bad) != none || p.decide(now.Add(120*time.Second), bad) != restartControl {
		t.Fatal("restart backoff was not doubled")
	}
	for i := 0; i < 20; i++ {
		next := p.next
		if p.decide(next, bad) != restartControl {
			t.Fatal("missing persistent recovery")
		}
		if p.next.Sub(next) > 5*time.Minute {
			t.Fatal("backoff exceeded the configured bound")
		}
	}
	if p.decide(p.next, Assessment{Status: Status{Status: "online"}}) != none || p.attempts != 0 {
		t.Fatal("healthy connectivity did not reset recovery")
	}
	now = p.next.Add(time.Hour)
	if p.decide(now, bad) != none || p.decide(now.Add(30*time.Second), bad) != rebind {
		t.Fatal("a new outage did not get a fresh grace/soft recovery")
	}
}
func TestTransportOnlyAndUnavailableNetworkNeverRestartControl(t *testing.T) {
	p := policy{config: DefaultConfig()}
	now := time.Unix(100, 0)
	transport := Assessment{Status: Status{Status: "error"}, Eligible: true, NetworkUp: true}
	p.decide(now, transport)
	for i := 0; i < 10; i++ {
		if got := p.decide(now.Add(time.Hour), transport); got != rebind {
			t.Fatalf("transport failure restarted control: %v", got)
		}
		now = now.Add(time.Hour)
	}
	if p.decide(now, Assessment{Status: Status{Status: "error"}}) != none || p.attempts != 0 {
		t.Fatal("network-down did not pause/reset recovery")
	}
	if p.decide(now, transport) != none || p.decide(now.Add(29*time.Second), transport) != none {
		t.Fatal("link-up did not receive a grace period")
	}
}
