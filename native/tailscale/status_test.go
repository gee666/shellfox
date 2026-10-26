package main

import "testing"

func TestLoginURLOnlyWhileAuthenticationIsRequired(t *testing.T) {
	url := "https://login.tailscale.com/a/test-login"
	for _, status := range []string{"online", "starting", "error", "disabled"} {
		if loginURLForState(status, url) != nil {
			t.Fatalf("stale login URL exposed for %s", status)
		}
	}
	if got := loginURLForState("login-required", url); got == nil || *got != url {
		t.Fatal("required login URL was not exposed")
	}
	for _, invalid := range []string{"", "http://login.tailscale.com/a/test", "https://other.example/a/test"} {
		if loginURLForState("login-required", invalid) != nil {
			t.Fatal("untrusted login URL exposed")
		}
	}
}
