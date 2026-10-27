package main

import (
	"testing"

	"shellfox.local/tailscale-helper/nethealth"
	"tailscale.com/ipn/ipnstate"
)

func TestLoginURLOnlyWhileAuthenticationIsRequired(t *testing.T) {
	url := "https://login.tailscale.com/a/test-login"
	for _, state := range []string{"Running", "Starting", "NoState", "Stopped", "Unknown"} {
		status := nethealth.Inspect(&ipnstate.Status{BackendState: state, AuthURL: url}, nil, nethealth.LinkUnknown)
		if status.LoginURL != nil {
			t.Fatalf("stale login URL exposed for %s", state)
		}
	}
	for _, state := range []string{"NeedsLogin", "NeedsMachineAuth"} {
		status := nethealth.Inspect(&ipnstate.Status{BackendState: state, AuthURL: url}, nil, nethealth.LinkUnknown)
		if status.LoginURL == nil || *status.LoginURL != url {
			t.Fatal("required login URL was not exposed")
		}
	}
	for _, invalid := range []string{"", "http://login.tailscale.com/a/test", "https://other.example/a/test"} {
		status := nethealth.Inspect(&ipnstate.Status{BackendState: "NeedsLogin", AuthURL: invalid}, nil, nethealth.LinkUnknown)
		if status.LoginURL != nil {
			t.Fatal("untrusted login URL exposed")
		}
	}
}
