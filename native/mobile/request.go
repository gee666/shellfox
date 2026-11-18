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

// RequestResult exposes immutable, gomobile-supported getters only.
// StatusCode is always a received HTTP status, never a translated error.
type RequestResult struct {
	statusCode int
	body       string
}

func (r *RequestResult) StatusCode() int { return r.statusCode }
func (r *RequestResult) Body() string    { return r.body }

// Request preserves the legacy string/error API and rejection behavior.
func Request(peerID, path, body string) (string, error) {
	response, err := RequestWithStatus(peerID, path, body)
	return legacyResponse(response, err)
}

// RequestWithStatus sends one allowlisted POST through tsnet. Both versions'
// events share the bounded poll pool. No OS HTTP/VPN, headers or URLs are accepted.
// Non-2xx bodies are discarded. Errors return nil, never a synthetic status or
// fallback decision; only the shared client may negotiate another profile.
func RequestWithStatus(peerID, path, body string) (*RequestResult, error) {
	if err := validateRequest(peerID, path, body); err != nil {
		return nil, err
	}
	n, err := current()
	if err != nil {
		return nil, err
	}
	release, err := n.admit(path)
	if err != nil {
		return nil, err
	}
	defer release()
	ctx, cancel := context.WithTimeout(n.ctx, requestTimeout)
	defer cancel()
	return n.transports.requestWithStatus(ctx, n.client, n.server.Dial, peerID, path, body)
}

func (n *node) admit(path string) (func(), error) {
	slots := n.requests
	if isEventPath(path) {
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
	response, err := p.requestWithStatus(ctx, client, dial, peerID, path, body)
	return legacyResponse(response, err)
}

func legacyResponse(response *RequestResult, err error) (string, error) {
	if err != nil {
		return "", err
	}
	if response.statusCode < 200 || response.statusCode >= 300 {
		return "", errors.New("Remote host rejected request")
	}
	return response.body, nil
}

func (p *transportPool) requestWithStatus(ctx context.Context, client peerClient, dial dialFunc, peerID, path, body string) (*RequestResult, error) {
	data, status, err := p.postPeer(ctx, client, dial, peerID, path, body, responseLimit)
	if err != nil {
		return nil, err
	}
	if status >= 200 && status < 300 && !json.Valid([]byte(data)) {
		return nil, errors.New("Invalid remote response")
	}
	return &RequestResult{statusCode: status, body: data}, nil
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
