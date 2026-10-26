package main

import (
	"context"
	"errors"
	"regexp"

	"tailscale.com/ipn"
	"tailscale.com/ipn/ipnstate"
)

type loginPayload struct {
	AuthKey string `json:"authKey,omitempty"`
}

type loginClient interface {
	StatusWithoutPeers(context.Context) (*ipnstate.Status, error)
	StartLoginInteractive(context.Context) error
	GetPrefs(context.Context) (*ipn.Prefs, error)
	Start(context.Context, ipn.Options) error
}

func validAuthKey(key string) bool {
	return key == "" || (len(key) <= 4096 && regexp.MustCompile(`^tskey-auth-[A-Za-z0-9_-]+$`).MatchString(key))
}

func enroll(ctx context.Context, client loginClient, key string) error {
	if !validAuthKey(key) {
		return errors.New("Invalid enrollment key")
	}
	if key == "" {
		if err := client.StartLoginInteractive(ctx); err != nil {
			return errors.New("Cannot start login")
		}
		return nil
	}
	status, err := client.StatusWithoutPeers(ctx)
	if err != nil || status == nil {
		return errors.New("Cannot read network status")
	}
	// Match tailscale up: Start installs the key but does not initiate fresh
	// authentication. Existing authorized node keys can resume without reauth.
	needsLogin := !status.HaveNodeKey || status.BackendState == "NeedsLogin"
	prefs, err := client.GetPrefs(ctx)
	if err != nil || prefs == nil {
		return errors.New("Cannot read network configuration")
	}
	prefs.WantRunning = true
	if err := client.Start(ctx, ipn.Options{UpdatePrefs: prefs, AuthKey: key}); err != nil {
		return errors.New("Cannot enroll node")
	}
	if needsLogin {
		if err := client.StartLoginInteractive(ctx); err != nil {
			return errors.New("Cannot start enrollment")
		}
	}
	return nil
}
