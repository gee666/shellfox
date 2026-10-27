package main

import (
	"context"
	"encoding/json"
	"errors"
	"net/netip"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"shellfox.local/tailscale-helper/nethealth"
	"tailscale.com/ipn"
	"tailscale.com/ipn/ipnstate"
)

func validConfig(t *testing.T) startConfig {
	t.Helper()
	return startConfig{StateDirectory: t.TempDir(), Hostname: "shellfox-laptop", BackendURL: "http://127.0.0.1:12345",
		BackendSecret: strings.Repeat("b", 32), ProxySecret: strings.Repeat("p", 32)}
}

func TestConfigAllowsOnlyLoopbackOriginAndValidSecrets(t *testing.T) {
	c := validConfig(t)
	for _, backend := range []string{"http://127.0.0.1:123", "http://[::1]:123/"} {
		c.BackendURL = backend
		if _, err := c.validate(); err != nil {
			t.Errorf("rejected %s: %v", backend, err)
		}
	}
	for _, backend := range []string{"https://127.0.0.1:123", "http://localhost:123", "http://192.168.1.1:123", "http://user:secret@127.0.0.1:123", "http://127.0.0.1:123/v1", "http://127.0.0.1:123?token=secret", "http://127.0.0.1:123/#frag", "http://127.0.0.1:0", "http://127.0.0.1", "http://[::ffff:127.0.0.1]:123"} {
		c.BackendURL = backend
		if _, err := c.validate(); err == nil {
			t.Errorf("accepted unsafe backend %s", backend)
		}
	}
	c = validConfig(t)
	c.StateDirectory = "relative"
	if _, err := c.validate(); err == nil {
		t.Error("relative state accepted")
	}
	c = validConfig(t)
	c.ProxySecret = c.BackendSecret
	if _, err := c.validate(); err == nil {
		t.Error("same secrets accepted")
	}
	for _, secret := range []string{"", "short", strings.Repeat("x", 257), strings.Repeat("x", 31) + "\n"} {
		if validSecret(secret) {
			t.Error("invalid secret accepted")
		}
	}
	c = validConfig(t)
	c.Hostname = "bad.host"
	if _, err := c.validate(); err == nil {
		t.Error("invalid hostname accepted")
	}
}

func TestStateDirectoryPersistsExistingIdentity(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "state")
	if err := prepareState(dir); err != nil {
		t.Fatal(err)
	}
	file := filepath.Join(dir, "tailscaled.state")
	if err := os.WriteFile(file, []byte("existing identity"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := prepareState(dir); err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(file)
	if err != nil || string(data) != "existing identity" {
		t.Fatal("existing identity was lost")
	}
	if err := prepareState(file); err == nil {
		t.Fatal("regular file accepted as state directory")
	}
}

func TestPeersAndAddressesFailClosed(t *testing.T) {
	for _, address := range []string{"100.64.0.1", "100.127.255.255", "fd7a:115c:a1e0::1"} {
		if !tailnetAddress(netip.MustParseAddr(address)) {
			t.Error("tailnet IP rejected")
		}
	}
	for _, address := range []string{"127.0.0.1", "10.0.0.1", "8.8.8.8", "::1", "100.128.0.1", "::ffff:100.64.0.1"} {
		if tailnetAddress(netip.MustParseAddr(address)) {
			t.Errorf("unsafe address accepted %s", address)
		}
	}
	f := testNetwork()
	if peers := ownPeers(f.status); len(peers) != 1 || peers[0].ID != "n-peer" || len(peers[0].Addresses) != 2 {
		t.Fatal("own peer missing")
	}
	f.status.Self.UserID = 999
	if len(ownPeers(f.status)) != 0 {
		t.Fatal("foreign peer exposed")
	}
	f.status.Self.UserID = 42
	for _, p := range f.status.Peer {
		p.TailscaleIPs = []netip.Addr{netip.MustParseAddr("127.0.0.1")}
	}
	if len(ownPeers(f.status)) != 0 {
		t.Fatal("non-tailnet address exposed")
	}
	if data, _ := json.Marshal(ownPeers(nil)); string(data) != "[]" {
		t.Fatal("empty peers must be an array")
	}
}

func TestLoginURLsAndProtocolValidation(t *testing.T) {
	if !nethealth.SafeLoginURL("https://login.tailscale.com/a/abc") {
		t.Fatal("valid login URL rejected")
	}
	for _, u := range []string{"", "http://login.tailscale.com/a", "https://evil.test/a", "https://login.tailscale.com.evil.test/a", "https://user@login.tailscale.com/a", "https://login.tailscale.com:443/a"} {
		if nethealth.SafeLoginURL(u) {
			t.Errorf("unsafe login URL %s", u)
		}
	}
	for _, id := range []string{`"request"`, `42`} {
		if !validID(json.RawMessage(id)) {
			t.Error("valid ID rejected")
		}
	}
	for _, id := range []string{`null`, `{}`, `true`, `[]`, ``} {
		if validID(json.RawMessage(id)) {
			t.Error("invalid ID accepted")
		}
	}
	var p loginPayload
	if decode([]byte(`{"authKey":"key","unexpected":true}`), &p) == nil || decode([]byte(`{} {}`), &p) == nil {
		t.Error("invalid JSON accepted")
	}
}

type fakeLogin struct {
	interactive bool
	prefs       *ipn.Prefs
	options     ipn.Options
	err         error
	status      *ipnstate.Status
	calls       []string
	failures    map[string]error
}

func (f *fakeLogin) call(method string) error {
	f.calls = append(f.calls, method)
	if err := f.failures[method]; err != nil {
		return err
	}
	return f.err
}
func (f *fakeLogin) StatusWithoutPeers(context.Context) (*ipnstate.Status, error) {
	status := f.status
	if status == nil {
		status = &ipnstate.Status{BackendState: "NeedsLogin"}
	}
	return status, f.call("status")
}
func (f *fakeLogin) StartLoginInteractive(context.Context) error {
	f.interactive = true
	return f.call("login")
}
func (f *fakeLogin) GetPrefs(context.Context) (*ipn.Prefs, error) { return f.prefs, f.call("prefs") }
func (f *fakeLogin) Start(_ context.Context, opts ipn.Options) error {
	f.options = opts
	return f.call("start")
}

func TestLoginAcceptsKeyAfterStartupWithoutPersistingIt(t *testing.T) {
	ctx := context.Background()
	f := &fakeLogin{prefs: ipn.NewPrefs()}
	f.prefs.Hostname = "existing-host"
	key := "tskey" + "-auth-" + "DO-NOT-ECHO"
	if err := enroll(ctx, f, key); err != nil {
		t.Fatal(err)
	}
	if f.options.AuthKey != key || !f.options.UpdatePrefs.WantRunning || f.options.UpdatePrefs.Hostname != "existing-host" || !f.interactive {
		t.Fatal("wrong enrollment options")
	}
	data, _ := json.Marshal(f.prefs)
	if strings.Contains(string(data), key) {
		t.Fatal("key persisted in preferences")
	}
	if err := enroll(ctx, f, ""); err != nil || !f.interactive {
		t.Fatal("interactive login not supported")
	}
	for _, key := range []string{"file:/private/key", "tskey" + "-client-" + "synthetic-secret", "bad\nkey", strings.Repeat("x", 4097)} {
		if err := enroll(ctx, f, key); err == nil || strings.Contains(err.Error(), key) {
			t.Fatal("invalid key accepted or leaked")
		}
	}
	f.err = errors.New(key)
	if err := enroll(ctx, f, key); err == nil || strings.Contains(err.Error(), key) {
		t.Fatal("upstream secret leaked")
	}
}
