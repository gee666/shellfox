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
		if err := enroll(context.Background(), f, "tskey" + "-auth-" + "synthetic-secret"); err != nil {
			t.Fatal(err)
		}
		if !reflect.DeepEqual(f.calls, []string{"status", "prefs", "start", "login"}) {
			t.Fatalf("incorrect enrollment order: %v", f.calls)
		}
		if f.options.AuthKey != "tskey" + "-auth-" + "synthetic-secret" || !f.options.UpdatePrefs.WantRunning {
			t.Fatal("authentication started without enrollment configuration")
		}
	}
}

func TestKeyEnrollmentPreservesExistingAuthorizedNode(t *testing.T) {
	for _, state := range []string{"Running", "Stopped", "NeedsMachineAuth"} {
		f := &fakeLogin{prefs: ipn.NewPrefs(), status: &ipnstate.Status{BackendState: state, HaveNodeKey: true}}
		if err := enroll(context.Background(), f, "tskey" + "-auth-" + "synthetic-secret"); err != nil {
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

func TestInteractiveLoginDoesNotRestartControlClient(t *testing.T) {
	f := &fakeLogin{}
	if err := enroll(context.Background(), f, ""); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(f.calls, []string{"login"}) {
		t.Fatalf("interactive login changed configuration: %v", f.calls)
	}
}
