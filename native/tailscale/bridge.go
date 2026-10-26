package main

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"net"
	"net/http"
	"net/netip"
	"net/url"
	"strconv"
	"strings"
	"time"

	"tailscale.com/client/tailscale/apitype"
	"tailscale.com/ipn/ipnstate"
)

type networkClient interface {
	Status(context.Context) (*ipnstate.Status, error)
	WhoIs(context.Context, string) (*apitype.WhoIsResponse, error)
}

type bridge struct {
	network       networkClient
	dial          func(context.Context, string, string) (net.Conn, error)
	backend       *url.URL
	backendSecret string
	proxySecret   string
}

type proxyRequest struct {
	PeerID string          `json:"peerId"`
	Path   string          `json:"path"`
	Body   json.RawMessage `json:"body"`
}

func allowedPath(path string) bool {
	switch path {
	case "/v1/pair", "/v1/request", "/v1/events", "/v1/disconnect":
		return true
	}
	return false
}

func readBody(w http.ResponseWriter, r *http.Request) ([]byte, bool) {
	body, err := io.ReadAll(http.MaxBytesReader(w, r.Body, bodyLimit))
	if err != nil {
		http.Error(w, "Body too large or unreadable", http.StatusRequestEntityTooLarge)
		return nil, false
	}
	return body, true
}

func validPost(w http.ResponseWriter, r *http.Request) bool {
	if r.Method != http.MethodPost {
		http.Error(w, "POST required", http.StatusMethodNotAllowed)
		return false
	}
	if r.URL.RawQuery != "" || r.URL.RawPath != "" || r.URL.Fragment != "" || r.URL.IsAbs() {
		http.Error(w, "Invalid request URL", http.StatusBadRequest)
		return false
	}
	if media := strings.TrimSpace(strings.Split(r.Header.Get("Content-Type"), ";")[0]); !strings.EqualFold(media, "application/json") {
		http.Error(w, "JSON required", http.StatusUnsupportedMediaType)
		return false
	}
	return true
}

func (b *bridge) outgoing(w http.ResponseWriter, r *http.Request) {
	// This listener is loopback-only. A token also blocks local websites and
	// unrelated processes from using the embedded node as a general proxy.
	if subtle.ConstantTimeCompare([]byte(r.Header.Get("Authorization")), []byte("Bearer "+b.proxySecret)) != 1 {
		http.Error(w, "Unauthorized", http.StatusUnauthorized)
		return
	}
	if !validPost(w, r) {
		return
	}
	if r.URL.Path != "/proxy" {
		http.NotFound(w, r)
		return
	}
	body, ok := readBody(w, r)
	if !ok {
		return
	}
	var p proxyRequest
	if decode(body, &p) != nil || p.PeerID == "" || len(p.PeerID) > 256 || !allowedPath(p.Path) || len(p.Body) == 0 {
		http.Error(w, "Invalid proxy request", http.StatusBadRequest)
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), requestTimeout)
	defer cancel()
	status, err := b.network.Status(ctx)
	if err != nil {
		http.Error(w, "Network unavailable", http.StatusServiceUnavailable)
		return
	}
	var address string
	for _, peer := range ownPeers(status) {
		if peer.ID == p.PeerID && peer.Online {
			address = net.JoinHostPort(peer.Addresses[0], servicePort)
			break
		}
	}
	if address == "" {
		http.Error(w, "Peer unavailable", http.StatusForbidden)
		return
	}
	// A dedicated transport cannot reuse a connection after peer ownership changes.
	transport := &http.Transport{Proxy: nil, DialContext: func(ctx context.Context, network, target string) (net.Conn, error) {
		if network != "tcp" || target != address {
			return nil, errors.New("invalid tailnet target")
		}
		return b.dial(ctx, "tcp", address)
	}, DisableKeepAlives: true, ResponseHeaderTimeout: requestTimeout, MaxResponseHeaderBytes: 16 * 1024}
	defer transport.CloseIdleConnections()
	forward(w, ctx, transport, "http://"+address+p.Path, p.Body, nil)
}

func (b *bridge) incoming(w http.ResponseWriter, r *http.Request) {
	if !validPost(w, r) {
		return
	}
	if !allowedPath(r.URL.Path) {
		http.NotFound(w, r)
		return
	}
	ctx, cancel := context.WithTimeout(r.Context(), requestTimeout)
	defer cancel()
	identity, err := b.incomingIdentity(ctx, r.RemoteAddr)
	if err != nil {
		http.Error(w, "Peer not authorized", http.StatusForbidden)
		return
	}
	body, ok := readBody(w, r)
	if !ok {
		return
	}
	if !json.Valid(body) {
		http.Error(w, "Invalid JSON", http.StatusBadRequest)
		return
	}
	// Start with empty headers. No caller-supplied identity, authorization,
	// forwarding, cookies or connection headers can reach the Node service.
	headers := signedHeaders(b.backendSecret, identity, r.Method, r.URL.Path, body, time.Now())
	transport := &http.Transport{Proxy: nil, DialContext: (&net.Dialer{Timeout: 5 * time.Second}).DialContext,
		DisableKeepAlives: true, ResponseHeaderTimeout: requestTimeout, MaxResponseHeaderBytes: 16 * 1024}
	defer transport.CloseIdleConnections()
	forward(w, ctx, transport, b.backend.Scheme+"://"+b.backend.Host+r.URL.Path, body, headers)
}

func (b *bridge) incomingIdentity(ctx context.Context, remote string) (peer, error) {
	host, port, err := net.SplitHostPort(remote)
	ip, ipErr := netip.ParseAddr(host)
	if err != nil || ipErr != nil || port == "" || !tailnetAddress(ip) {
		return peer{}, errors.New("invalid remote address")
	}
	who, err := b.network.WhoIs(ctx, remote)
	if err != nil || who == nil || who.Node == nil || who.UserProfile == nil || who.Node.StableID == "" || len(who.Node.Tags) != 0 {
		return peer{}, errors.New("unknown peer")
	}
	st, err := b.network.Status(ctx)
	if err != nil || st == nil || st.Self == nil || st.BackendState != "Running" || st.Self.UserID == 0 || who.Node.User != st.Self.UserID || who.UserProfile.ID != st.Self.UserID {
		return peer{}, errors.New("peer not owned by current user")
	}
	// WhoIs proves the address, but the current peer map must agree too.
	for _, p := range ownPeers(st) {
		if p.ID == string(who.Node.StableID) {
			for _, address := range p.Addresses {
				if address == ip.String() {
					return p, nil
				}
			}
		}
	}
	return peer{}, errors.New("peer not in current network map")
}

func signedHeaders(secret string, p peer, method, path string, body []byte, now time.Time) http.Header {
	identity, _ := json.Marshal(struct {
		ID       string `json:"id"`
		Hostname string `json:"hostname"`
	}{p.ID, p.Hostname})
	peerHeader := base64.RawURLEncoding.EncodeToString(identity)
	timestamp := strconv.FormatInt(now.UnixMilli(), 10)
	digest := sha256.Sum256(body)
	mac := hmac.New(sha256.New, []byte(secret))
	_, _ = io.WriteString(mac, timestamp+"\n"+peerHeader+"\n"+method+"\n"+path+"\n"+hex.EncodeToString(digest[:]))
	return http.Header{
		"X-Shellfox-Peer": {peerHeader}, "X-Shellfox-Timestamp": {timestamp}, "X-Shellfox-Signature": {hex.EncodeToString(mac.Sum(nil))},
	}
}

func forward(w http.ResponseWriter, ctx context.Context, transport *http.Transport, target string, body []byte, headers http.Header) {
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, target, bytes.NewReader(body))
	if err != nil {
		http.Error(w, "Invalid upstream request", http.StatusBadGateway)
		return
	}
	if headers != nil {
		req.Header = headers
	}
	req.Header.Set("Content-Type", "application/json")
	client := &http.Client{Transport: transport, Timeout: requestTimeout,
		CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	response, err := client.Do(req)
	if err != nil {
		http.Error(w, "Upstream unavailable", http.StatusBadGateway)
		return
	}
	defer response.Body.Close()
	data, err := io.ReadAll(io.LimitReader(response.Body, responseLimit+1))
	if err != nil || len(data) > responseLimit {
		http.Error(w, "Invalid upstream response", http.StatusBadGateway)
		return
	}
	// Never relay Location, Set-Cookie or upstream identity headers.
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("Cache-Control", "no-store")
	w.WriteHeader(response.StatusCode)
	_, _ = w.Write(data)
}
