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

// Request posts JSON to one of the four Shellfox endpoints through tsnet.
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
	return request(ctx, n.client, n.server.Dial, peerID, path, body)
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

func request(ctx context.Context, client peerClient, dial dialFunc, peerID, path, body string) (string, error) {
	if err := validateRequest(peerID, path, body); err != nil {
		return "", err
	}
	st, err := client.Status(ctx)
	if err != nil {
		return "", errors.New("Network unavailable")
	}
	address := ""
	for _, peer := range ownPeers(st) {
		if peer.ID == peerID && peer.Online {
			address = net.JoinHostPort(peer.Addresses[0], servicePort)
			break
		}
	}
	if address == "" {
		return "", errors.New("Peer unavailable or not owned by current user")
	}
	// Re-read ownership per request. Never reuse a connection across a map change.
	transport := &http.Transport{
		Proxy: nil, DisableKeepAlives: true, DisableCompression: true,
		ResponseHeaderTimeout: requestTimeout, MaxResponseHeaderBytes: 16 * 1024,
		DialContext: func(ctx context.Context, network, target string) (net.Conn, error) {
			if network != "tcp" || target != address {
				return nil, errors.New("Invalid tailnet target")
			}
			return dial(ctx, "tcp", address)
		},
	}
	defer transport.CloseIdleConnections()
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, "http://"+address+path, strings.NewReader(body))
	if err != nil {
		return "", errors.New("Invalid upstream request")
	}
	req.Header.Set("Content-Type", "application/json")
	httpClient := &http.Client{Transport: transport, Timeout: requestTimeout,
		CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	response, err := httpClient.Do(req)
	if err != nil {
		return "", errors.New("Remote host unavailable")
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return "", errors.New("Remote host rejected request")
	}
	data, err := io.ReadAll(io.LimitReader(response.Body, responseLimit+1))
	if err != nil || len(data) > responseLimit || !json.Valid(data) {
		return "", errors.New("Invalid remote response")
	}
	return string(data), nil
}
