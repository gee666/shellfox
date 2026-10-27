package nethealth

import (
	"testing"

	"tailscale.com/health"
	"tailscale.com/ipn/ipnstate"
	"tailscale.com/tsconst"
)

func connected() *ipnstate.Status {
	return &ipnstate.Status{BackendState: "Running", HaveNodeKey: true, Self: &ipnstate.PeerStatus{Online: true}}
}
func warning(code health.WarnableCode, affects bool) *health.State {
	return &health.State{Warnings: map[health.WarnableCode]health.UnhealthyState{code: {ImpactsConnectivity: affects, Text: "private upstream credentials must not escape"}}}
}
func TestRunningIsNotControlConnectivity(t *testing.T) {
	st := connected()
	st.Self.Online = false
	a := Inspect(st, nil, LinkUp)
	if a.Status.Status != "error" || !a.ControlLost || !a.Eligible || a.Error == nil {
		t.Fatalf("false online: %+v", a)
	}
	st.Self.Online = true
	for _, code := range []health.WarnableCode{tsconst.HealthWarnableMapResponseTimeout, tsconst.HealthWarnableNotInMapPoll} {
		a = Inspect(st, warning(code, false), LinkUp)
		if a.Status.Status != "error" || !a.ControlLost || !a.Eligible {
			t.Fatalf("stale Self.Online accepted for %s: %+v", code, a)
		}
		if *a.Error != "Tailscale control connection unavailable. Reconnecting automatically." {
			t.Fatal("raw SDK warning escaped")
		}
	}
	if a = Inspect(st, nil, LinkUp); a.Status.Status != "online" || a.Eligible {
		t.Fatalf("did not recover: %+v", a)
	}
}
func TestUnrelatedWarningsDoNotRestartHealthyControl(t *testing.T) {
	for _, code := range []health.WarnableCode{tsconst.HealthWarnableUpdateAvailable, tsconst.HealthWarnableSecurityUpdateAvailable, tsconst.HealthWarnableLocalLogConfigError, tsconst.HealthWarnableWarmingUp} {
		a := Inspect(connected(), warning(code, false), LinkUp)
		if a.Status.Status != "online" || a.Eligible {
			t.Fatalf("unrelated warning %s caused recovery: %+v", code, a)
		}
	}
	a := Inspect(connected(), warning("control-health.account-policy", true), LinkUp)
	if a.Status.Status != "online" || a.Error == nil || a.Eligible {
		t.Fatal("unknown control-plane policy must be reported as degraded, not restarted")
	}
}
func TestLinkAndTransportFailuresHaveDifferentRecovery(t *testing.T) {
	for _, code := range []health.WarnableCode{tsconst.HealthWarnableNoDERPConnection, tsconst.HealthWarnableNoUDP4Bind, tsconst.HealthWarnableMagicsockReceiveFuncError} {
		a := Inspect(connected(), warning(code, true), LinkUp)
		if a.Status.Status != "online" || a.Error == nil || a.ControlLost || !a.Eligible {
			t.Fatalf("wrong transport assessment: %+v", a)
		}
	}
	for _, a := range []Assessment{Inspect(connected(), nil, LinkDown), Inspect(connected(), warning(tsconst.HealthWarnableNetworkStatus, true), LinkUp)} {
		if a.Status.Status != "error" || a.NetworkUp || a.Eligible {
			t.Fatalf("recovery ran without an uplink: %+v", a)
		}
	}
}
func TestUnauthorizedStoppedAndMissingStatusNeverRestart(t *testing.T) {
	for _, backend := range []string{"NeedsLogin", "NeedsMachineAuth", "Stopped", "NoState"} {
		st := connected()
		st.BackendState = backend
		st.AuthURL = "https://login.tailscale.com/a/test"
		a := Inspect(st, nil, LinkUp)
		if a.Eligible || a.Status.Status == "online" {
			t.Fatalf("unexpected recovery for %s", backend)
		}
		if backend == "Stopped" && a.LoginURL != nil {
			t.Fatal("a stopped authorized node must not request enrollment")
		}
	}
	if a := Inspect(nil, nil, LinkUp); a.Status.Status != "error" || a.Eligible {
		t.Fatal("missing status was considered healthy")
	}
	st := connected()
	st.HaveNodeKey = false
	st.Self.Online = false
	if Inspect(st, nil, LinkUp).Eligible {
		t.Fatal("recovery must not create a new node identity")
	}
}
