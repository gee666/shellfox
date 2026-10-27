package nethealth

import (
	"testing"

	"tailscale.com/tsconst"
)

func TestUnknownMobileLinkDoesNotOverrideLiveControl(t *testing.T) {
	a := Inspect(connected(), nil, LinkUnknown)
	if a.Status.Status != "online" || a.Error != nil || a.Eligible {
		t.Fatalf("unknown enumeration broke a working control connection: %+v", a)
	}
	a = Inspect(connected(), warning(tsconst.HealthWarnableNetworkStatus, true), LinkUnknown)
	if a.Status.Status != "error" || a.NetworkUp || a.Eligible {
		t.Fatal("an authoritative SDK network-down warning was ignored")
	}
	st := connected()
	st.Self.Online = false
	a = Inspect(st, nil, LinkUnknown)
	if a.Status.Status != "error" || !a.Eligible {
		t.Fatal("missing interface metadata prevented recovery of proven control loss")
	}
}
