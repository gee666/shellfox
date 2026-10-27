package main

import (
	"context"
	"encoding/json"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"sync"
	"testing"
)

func TestApprovalForwardingPreservesPairAttemptAndDoesNotConfuseDiscovery(t *testing.T) {
	const attempt = "10000000-0000-4000-8000-000000000001"
	const waiting = `{"ok":false,"error":{"code":"APPROVAL_REQUIRED","message":"Approve on host.","retryable":true}}`
	pending := map[string]bool{}
	var mu sync.Mutex
	count := func() int { mu.Lock(); defer mu.Unlock(); return len(pending) }
	backend := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("X-Shellfox-Peer") == "" || r.Header.Get("X-Shellfox-Signature") == "" {
			t.Error("approval request lost verified identity")
		}
		if r.URL.Path == "/v1/discover" {
			_, _ = io.WriteString(w, `{"ok":true,"value":{"service":"shellfox","protocolVersion":1}}`)
			return
		}
		var body struct {
			ClientID string `json:"clientId"`
		}
		if json.NewDecoder(r.Body).Decode(&body) != nil || body.ClientID != attempt {
			t.Error("pair or disconnect changed the active attempt UUID")
		}
		mu.Lock()
		defer mu.Unlock()
		switch r.URL.Path {
		case "/v1/pair":
			pending[body.ClientID] = true
			_, _ = io.WriteString(w, waiting)
		case "/v1/disconnect":
			delete(pending, body.ClientID)
			_, _ = io.WriteString(w, `{"ok":true,"value":{"disconnected":true}}`)
		default:
			t.Errorf("unexpected approval route %s", r.URL.Path)
		}
	}))
	defer backend.Close()
	backendURL, _ := url.Parse(backend.URL)
	incoming := &bridge{network: testNetwork(), backend: backendURL, backendSecret: strings.Repeat("b", 32)}
	tailnet := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		r.RemoteAddr = "100.100.1.2:12345"
		incoming.incoming(w, r)
	}))
	defer tailnet.Close()
	tailnetURL, _ := url.Parse(tailnet.URL)
	outgoing := &bridge{network: testNetwork(), proxySecret: strings.Repeat("p", 32), dial: func(ctx context.Context, network, address string) (net.Conn, error) {
		return (&net.Dialer{}).DialContext(ctx, "tcp", tailnetURL.Host)
	}}
	call := func(path string) string {
		body := `{"peerId":"n-peer","path":"` + path + `","body":{"clientId":"` + attempt + `"}}`
		response := proxyCall(t, outgoing, body, "Bearer "+outgoing.proxySecret)
		if response.Code != http.StatusOK {
			t.Fatalf("%s failed: %d %s", path, response.Code, response.Body.String())
		}
		return response.Body.String()
	}
	call("/v1/discover")
	if count() != 0 {
		t.Fatal("discovery created an approval request")
	}
	for i := 0; i < 2; i++ {
		if result := call("/v1/pair"); result != waiting || count() != 1 {
			t.Fatalf("approval reply or retry changed: %s pending=%d", result, count())
		}
	}
	call("/v1/disconnect")
	if count() != 0 {
		t.Fatal("explicit cancel did not reach the pending attempt")
	}
}
