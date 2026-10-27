package nethealth

import (
	"testing"
	"time"
)

func TestFreshControlLossNeverInheritsTransportGraceOrBackoff(t *testing.T) {
	for _, long := range []bool{false, true} {
		p := policy{config: DefaultConfig()}
		now := time.Unix(100, 0)
		transport := Assessment{Status: Status{Status: "online"}, Eligible: true, NetworkUp: true}
		control := transport
		control.ControlLost = true
		control.Status.Status = "error"
		p.decide(now, transport)
		if p.decide(now.Add(30*time.Second), transport) != rebind {
			t.Fatal("transport repair missing")
		}
		if long {
			for i := 0; i < 10; i++ {
				p.decide(p.next, transport)
			}
			now = p.next.Add(-time.Second)
		} else {
			now = now.Add(59 * time.Second)
		}
		if p.decide(now, control) != none || p.decide(now.Add(time.Second), control) != none {
			t.Fatal("transport history accelerated a new control reset")
		}
		if p.decide(now.Add(30*time.Second), control) != rebind {
			t.Fatal("new control outage inherited long transport backoff")
		}
		if p.decide(now.Add(59*time.Second), control) != none || p.decide(now.Add(60*time.Second), control) != restartControl {
			t.Fatal("control restart did not require its own sixty-second outage")
		}
		if p.decide(now.Add(61*time.Second), transport) != none {
			t.Fatal("recovered control was reset for a remaining relay warning")
		}
	}
}
