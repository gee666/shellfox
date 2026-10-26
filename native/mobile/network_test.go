package mobile

import (
	"encoding/json"
	"errors"
	"net"
	"strings"
	"testing"
)

type fakeProvider struct {
	raw string
	err error
}

func (f fakeProvider) Interfaces() (string, error) { return f.raw, f.err }

func TestAndroidSDKInterfacesPreserveAddressesAndFlags(t *testing.T) {
	raw := `{"interfaces":[{"name":"wlan0","index":4,"mtu":1500,"up":true,"loopback":false,"pointToPoint":false,"multicast":true,"addresses":["192.168.1.10/24","2001:db8::123/64"]},{"name":"lo","index":1,"mtu":65536,"up":true,"loopback":true,"addresses":["127.0.0.1/8"]}],"defaultInterface":"wlan0","gateway":"192.168.1.1"}`
	snapshot, interfaces, err := decodeInterfaces(raw)
	if err != nil || len(interfaces) != 2 || snapshot.DefaultInterface != "wlan0" || snapshot.Gateway != "192.168.1.1" {
		t.Fatalf("snapshot %v", err)
	}
	wifi := interfaces[0]
	addresses, err := wifi.Addrs()
	if err != nil || wifi.Name != "wlan0" || wifi.Index != 4 || wifi.MTU != 1500 || !wifi.IsUp() || wifi.Flags&net.FlagMulticast == 0 {
		t.Fatal("interface properties lost")
	}
	if len(addresses) != 2 || addresses[0].String() != "192.168.1.10/24" || addresses[1].String() != "2001:db8::123/64" {
		t.Fatal("interface prefixes changed")
	}
	if !interfaces[1].IsLoopback() {
		t.Fatal("loopback flag lost")
	}
	// Use supplied SDK addresses, never Go Interface.Addrs, which opens netlink.
	empty := `{"interfaces":[{"name":"offline","addresses":[]}],"defaultInterface":"","gateway":""}`
	_, interfaces, err = decodeInterfaces(empty)
	if err != nil {
		t.Fatal(err)
	}
	if addresses, err := interfaces[0].Addrs(); err != nil || len(addresses) != 0 {
		t.Fatal("empty SDK list fell through to netlink")
	}
}

func TestAndroidInterfaceSnapshotRejectsMalformedAndUnboundedData(t *testing.T) {
	for _, raw := range []string{
		"{", strings.Repeat("x", 256*1024+1),
		`{"interfaces":[{"name":"","addresses":[]}]}`,
		`{"interfaces":[{"name":"wlan0","index":-1}]}`,
		`{"interfaces":[{"name":"wlan0","mtu":-1}]}`,
		`{"interfaces":[{"name":"wlan0","mtu":1048577}]}`,
		`{"interfaces":[{"name":"wlan0","addresses":["not-a-prefix"]}]}`,
		`{"interfaces":[{"name":"wlan0","addresses":["fe80::1%wlan0/64"]}]}`,
		`{"interfaces":[],"gateway":"not-an-IP"}`,
	} {
		if _, _, err := decodeInterfaces(raw); err == nil || err.Error() != "Cannot read Android network interfaces" {
			t.Fatalf("bad snapshot accepted or leaked: %v", err)
		}
	}
	tooMany := interfaceSnapshot{Interfaces: make([]interfaceInfo, 257)}
	raw, _ := json.Marshal(tooMany)
	if _, _, err := decodeInterfaces(string(raw)); err == nil {
		t.Fatal("too many interfaces")
	}
}

func TestAndroidProviderFailuresAreRedacted(t *testing.T) {
	defer SetNetworkProvider(nil)
	SetNetworkProvider(nil)
	if _, err := readInterfaces(); !errors.Is(err, errInterfaces) {
		t.Fatal("missing provider accepted")
	}
	SetNetworkProvider(fakeProvider{err: errors.New("tskey-auth-private-platform-error")})
	if _, err := readInterfaces(); !errors.Is(err, errInterfaces) || strings.Contains(err.Error(), "private") {
		t.Fatal("raw platform error leaked")
	}
	SetNetworkProvider(fakeProvider{raw: `{"interfaces":[]}`})
	if interfaces, err := readInterfaces(); err != nil || interfaces == nil {
		t.Fatal("valid offline network rejected")
	}
}
