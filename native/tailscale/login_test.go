package main

import (
	"context"
	"errors"
	"reflect"
	"strings"
	"testing"

	"tailscale.com/ipn"
	"tailscale.com/ipn/ipnstate"
)

func TestKeyEnrollmentStartsAuthenticationAfterInstallingKey(t *testing.T) {
	for _, state := range []ipnstate.Status{
		{BackendState: "NeedsLogin"},
		{BackendState: "Starting"},
		{BackendState: "NeedsLogin", HaveNodeKey: true},
	} {
		f := &fakeLogin{prefs: ipn.NewPrefs(), status: &state}
		if err := enroll(context.Background(), f, "tskey"+"-auth-"+"synthetic-secret"); err != nil {
			t.Fatal(err)
		}
		if !reflect.DeepEqual(f.calls, []string{"status", "prefs", "start", "login"}) {
			t.Fatalf("incorrect enrollment order: %v", f.calls)
		}
		if f.options.AuthKey != "tskey"+"-auth-"+"synthetic-secret" || !f.options.UpdatePrefs.WantRunning {
			t.Fatal("authentication started without enrollment configuration")
		}
	}
}

func TestKeyEnrollmentPreservesExistingAuthorizedNode(t *testing.T) {
	for _, state := range []string{"Running", "Stopped", "NeedsMachineAuth"} {
		f := &fakeLogin{prefs: ipn.NewPrefs(), status: &ipnstate.Status{BackendState: state, HaveNodeKey: true}}
		if err := enroll(context.Background(), f, "tskey"+"-auth-"+"synthetic-secret"); err != nil {
			t.Fatal(err)
		}
		if !reflect.DeepEqual(f.calls, []string{"status", "prefs", "start"}) {
			t.Fatalf("existing node unnecessarily reauthenticated: %v", f.calls)
		}
	}
}

func TestKeyEnrollmentStopsOnErrorsWithoutExposingSecrets(t *testing.T) {
	secret := "tskey" + "-auth-" + "DO-NOT-ECHO"
	for _, stage := range []string{"status", "prefs", "start", "login"} {
		t.Run(stage, func(t *testing.T) {
			f := &fakeLogin{prefs: ipn.NewPrefs(), failures: map[string]error{stage: errors.New("upstream rejected " + secret)}}
			err := enroll(context.Background(), f, secret)
			if err == nil || strings.Contains(err.Error(), secret) || strings.Contains(err.Error(), "upstream") {
				t.Fatal("enrollment failure missing or leaked upstream error")
			}
			if f.calls[len(f.calls)-1] != stage {
				t.Fatalf("continued after failed %s: %v", stage, f.calls)
			}
		})
	}
}

func TestInteractiveLoginPreservesRunningPreferences(t *testing.T) {
	prefs := ipn.NewPrefs()
	prefs.WantRunning = true
	f := &fakeLogin{prefs: prefs}
	if err := enroll(context.Background(), f, ""); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(f.calls, []string{"prefs", "start", "login"}) || f.options.UpdatePrefs != prefs {
		t.Fatal("interactive login replaced running preferences or skipped Start")
	}
}
func TestInteractiveLoginResumesNodeBeforeAuthenticationAndPreservesControl(t *testing.T) {
	prefs := ipn.NewPrefs()
	prefs.WantRunning = false
	prefs.ControlURL = "http://127.0.0.1:1234"
	f := &fakeLogin{prefs: prefs}
	if err := enroll(context.Background(), f, ""); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(f.calls, []string{"prefs", "start", "login"}) {
		t.Fatalf("incorrect interactive order: %v", f.calls)
	}
	if f.options.UpdatePrefs != prefs || !prefs.WantRunning || prefs.ControlURL != "http://127.0.0.1:1234" || f.options.AuthKey != "" {
		t.Fatal("interactive login replaced configuration or supplied a key")
	}
}
func TestInteractiveLoginStopsOnErrorsWithoutExposingUpstream(t *testing.T) {
	for _, stage := range []string{"prefs", "start", "login"} {
		f := &fakeLogin{prefs: ipn.NewPrefs(), failures: map[string]error{stage: errors.New("upstream secret")}}
		err := enroll(context.Background(), f, "")
		if err == nil || strings.Contains(err.Error(), "secret") || strings.Contains(err.Error(), "upstream") || f.calls[len(f.calls)-1] != stage {
			t.Fatal("interactive login continued after failure or leaked upstream content")
		}
	}
}
