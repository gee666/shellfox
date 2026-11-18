package mobile

import (
	"path/filepath"
	"testing"
)

func TestControlServerStartRejectsBeforeNodeOrStateCreation(t *testing.T) {
	for _, origin := range []string{"", "https://controlplane.tailscale.com", "http://localhost:1234", "http://127.0.0.1:0", "http://127.0.0.1:1234/path", "http://user@127.0.0.1:1234", "http://[::ffff:127.0.0.1]:1234"} {
		if err := StartWithControlServer(filepath.Join(t.TempDir(), "node"), "fixture", origin); err == nil {
			t.Fatalf("accepted invalid explicit origin %q", origin)
		}
	}
	// Valid loopback parsing still goes through ordinary state/hostname validation.
	if err := StartWithControlServer("relative", "fixture", "http://127.0.0.1:1234"); err == nil {
		t.Fatal("accepted relative state")
	}
	if err := StartWithControlServer(t.TempDir(), "invalid name", "http://[::1]:1234"); err == nil {
		t.Fatal("accepted invalid hostname")
	}
	singleton.Lock()
	defer singleton.Unlock()
	if singleton.node != nil {
		t.Fatal("validation created a node")
	}
}
