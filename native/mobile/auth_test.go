package mobile

import (
	"context"
	"errors"
	"strings"
	"testing"

	"tailscale.com/ipn"
	"tailscale.com/ipn/ipnstate"
)

type fakeLogin struct {
	st                  *ipnstate.Status
	starts, interactive int
	got                 ipn.Options
	fail                string
}

func (f *fakeLogin) StatusWithoutPeers(context.Context) (*ipnstate.Status, error) {
	if f.fail == "status" {
		return nil, errors.New("tskey" + "-auth-" + "synthetic-secret")
	}
	return f.st, nil
}
func (f *fakeLogin) StartLoginInteractive(context.Context) error {
	f.interactive++
	if f.fail == "interactive" {
		return errors.New("tskey" + "-auth-" + "synthetic-secret")
	}
	return nil
}
func (f *fakeLogin) GetPrefs(context.Context) (*ipn.Prefs, error) {
	if f.fail == "prefs" {
		return nil, errors.New("tskey" + "-auth-" + "synthetic-secret")
	}
	return &ipn.Prefs{}, nil
}
func (f *fakeLogin) Start(_ context.Context, o ipn.Options) error {
	f.starts++
	f.got = o
	if f.fail == "start" {
		return errors.New("tskey" + "-auth-" + "synthetic-secret")
	}
	return nil
}

func TestEnrollmentAndInteractiveLogin(t *testing.T) {
	for _, existing := range []bool{false, true} {
		f := &fakeLogin{st: &ipnstate.Status{BackendState: "Running", HaveNodeKey: existing}}
		if err := enroll(context.Background(), f, "tskey" + "-auth-" + "synthetic-secret"); err != nil {
			t.Fatal(err)
		}
		if f.starts != 1 || f.got.AuthKey != "tskey" + "-auth-" + "synthetic-secret" || !f.got.UpdatePrefs.WantRunning {
			t.Fatal("key not installed")
		}
		expected := 1
		if existing {
			expected = 0
		}
		if f.interactive != expected {
			t.Fatal("wrong fresh-login behavior")
		}
	}
	f := &fakeLogin{}
	if err := enroll(context.Background(), f, ""); err != nil || f.interactive != 1 || f.starts != 0 {
		t.Fatal("interactive login failed")
	}
	f = &fakeLogin{st: &ipnstate.Status{BackendState: "NeedsLogin", HaveNodeKey: true}}
	if err := enroll(context.Background(), f, "tskey" + "-auth-" + "synthetic-secret"); err != nil || f.interactive != 1 {
		t.Fatal("expired key not reauthenticated")
	}
}

func TestEnrollmentValidationAndRedactedFailures(t *testing.T) {
	for _, value := range []string{"bad", "tskey-auth-", "tskey" + "-auth-" + "synthetic-secret\n", "tskey" + "-client-" + "synthetic-secret", "tskey-auth-" + strings.Repeat("a", 4096)} {
		f := &fakeLogin{}
		if err := enroll(context.Background(), f, value); err == nil || f.starts != 0 || f.interactive != 0 {
			t.Fatal("invalid key accepted")
		}
	}
	for _, phase := range []string{"status", "prefs", "start", "interactive"} {
		f := &fakeLogin{st: &ipnstate.Status{}, fail: phase}
		err := enroll(context.Background(), f, "tskey" + "-auth-" + "synthetic-secret")
		if err == nil || strings.Contains(err.Error(), "secret") {
			t.Fatal("key or raw SDK error exposed")
		}
	}
}

func TestStatusNeverExposesStaleOrUntrustedLoginURL(t *testing.T) {
	for _, raw := range []string{"http://login.tailscale.com/a", "https://evil.test/a", "https://login.tailscale.com.evil.test/a", "https://user@login.tailscale.com/a", "https://login.tailscale.com:443/a"} {
		if safeLoginURL(raw) {
			t.Fatalf("unsafe URL %s", raw)
		}
	}
	for _, state := range []string{"Running", "Starting", "NoState", "SomethingElse"} {
		if statusFor(&ipnstate.Status{BackendState: state, AuthURL: "https://login.tailscale.com/a/secret"}).LoginURL != nil {
			t.Fatal("stale login URL")
		}
	}
	st := statusFor(&ipnstate.Status{BackendState: "NeedsLogin", AuthURL: "https://login.tailscale.com/a/test"})
	if st.LoginURL == nil || st.Status != "login-required" {
		t.Fatal("missing browser URL")
	}
}

func TestStoppedAPIAndInvalidStart(t *testing.T) {
	if err := Stop(); err != nil {
		t.Fatal(err)
	}
	if err := Logout(); err != nil {
		t.Fatal(err)
	}
	if status, err := Status(); err != nil || status != `{"status":"disabled","loginUrl":null,"error":null}` {
		t.Fatalf("%s %v", status, err)
	}
	if _, err := Peers(); err == nil {
		t.Fatal("stopped peers succeeded")
	}
	if err := Login(""); err == nil {
		t.Fatal("stopped login succeeded")
	}
	if _, err := Request("n-peer", "/v1/request", "{}"); err == nil {
		t.Fatal("stopped request succeeded")
	}
	for _, cfg := range [][2]string{{"relative", "good"}, {"", "good"}, {t.TempDir(), "bad.host"}, {t.TempDir(), "-bad"}, {t.TempDir(), strings.Repeat("x", 64)}} {
		if err := Start(cfg[0], cfg[1]); err == nil {
			t.Fatal("invalid config accepted")
		}
	}
}
