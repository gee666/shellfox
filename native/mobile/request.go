package mobile

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"net/http"
	"strings"

	"tailscale.com/ipn/ipnstate"
)

type peerClient interface {
	Status(context.Context) (*ipnstate.Status, error)
}
type dialFunc func(context.Context, string, string) (net.Conn, error)

// Request posts JSON to an allowlisted Shellfox endpoint through tsnet.
// /v1/events is a bounded long-poll, so it must run concurrently with requests.
// WireGuard encrypts this HTTP traffic; it never goes through OS HTTP or a VPN.
func Request(peerID, path, body string) (string, error) {
	if err := validateRequest(peerID, path, body); err != nil {
		return "", err
	}
	n, err := current()
	if err != nil {
		return "", err
	}
	release, err := n.admit(path)
	if err != nil {
		return "", err
	}
	defer release()
	ctx, cancel := context.WithTimeout(n.ctx, requestTimeout)
	defer cancel()
	return n.transports.request(ctx, n.client, n.server.Dial, peerID, path, body)
}

func (n *node) admit(path string) (func(), error) {
	slots := n.requests
	if path == "/v1/events" {
		slots = n.polls
	}
	select {
	case slots <- struct{}{}:
		return func() { <-slots }, nil
	default:
		return nil, errors.New("Too many pending network requests")
	}
}

func validateRequest(peerID, path, body string) error {
	if peerID == "" || len(peerID) > 256 || !allowedPath(path) || len(body) == 0 || len(body) > bodyLimit || !json.Valid([]byte(body)) {
		return errors.New("Invalid network request")
	}
	return nil
}

func (p *transportPool) request(ctx context.Context, client peerClient, dial dialFunc, peerID, path, body string) (string, error) {
	data, status, err := p.postPeer(ctx, client, dial, peerID, path, body, responseLimit)
	if err != nil {
		return "", err
	}
	if status < 200 || status >= 300 {
		return "", errors.New("Remote host rejected request")
	}
	if !json.Valid([]byte(data)) {
		return "", errors.New("Invalid remote response")
	}
	return data, nil
}

func postPeer(ctx context.Context, client peerClient, dial dialFunc, peerID, path, body string, limit int64) (string, int, error) {
	pool := &transportPool{}
	defer pool.close()
	return pool.postPeer(ctx, client, dial, peerID, path, body, limit)
}

func (p *transportPool) postPeer(ctx context.Context, client peerClient, dial dialFunc, peerID, path, body string, limit int64) (string, int, error) {
	if err := validateRequest(peerID, path, body); err != nil {
		return "", 0, err
	}
	transport, address, err := p.transport(ctx, client, dial, peerID)
	if err != nil {
		return "", 0, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, "http://"+address+path, strings.NewReader(body))
	if err != nil {
		return "", 0, errors.New("Invalid upstream request")
	}
	// A failed POST is uncertain, even on a reused connection. Do not let the
	// standard transport replay it after a stale idle connection write failure.
	req.GetBody = nil
	req.Header.Set("Content-Type", "application/json")
	httpClient := &http.Client{Transport: transport, Timeout: requestTimeout,
		CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	response, err := httpClient.Do(req)
	if err != nil {
		return "", 0, errors.New("Remote host unavailable")
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return "", response.StatusCode, nil
	}
	data, err := io.ReadAll(io.LimitReader(response.Body, limit+1))
	if err != nil || int64(len(data)) > limit {
		return "", 0, errors.New("Invalid remote response")
	}
	return string(data), response.StatusCode, nil
}
