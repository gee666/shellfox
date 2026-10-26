package mobile

import (
	"context"
	"errors"
	"regexp"

	"tailscale.com/ipn"
	"tailscale.com/ipn/ipnstate"
)

type loginClient interface {
	StatusWithoutPeers(context.Context) (*ipnstate.Status, error)
	StartLoginInteractive(context.Context) error
	GetPrefs(context.Context) (*ipn.Prefs, error)
	Start(context.Context, ipn.Options) error
}

// Login accepts a user-owned enrollment auth key, or an empty string for browser
// login. Status exposes only validated Tailscale HTTPS login URLs.
// Auth keys are never logged, returned, or saved in app preferences.
func Login(authKey string) error {
	n, err := current()
	if err != nil {
		return err
	}
	ctx, cancel := context.WithTimeout(n.ctx, requestTimeout)
	defer cancel()
	return enroll(ctx, n.client, authKey)
}

func enroll(ctx context.Context, client loginClient, key string) error {
	if key != "" && (len(key) > 4096 || !regexp.MustCompile(`^tskey-auth-[A-Za-z0-9_-]+$`).MatchString(key)) {
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
