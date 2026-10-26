package main

import (
	"context"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strconv"
	"strings"
	"testing"
	"time"
)

func TestDiscoveryAllowlistAddsNoGeneralProxyPaths(t *testing.T) {
	if !allowedPath("/v1/discover") {
		t.Fatal("discovery not allowed")
	}
	for _, path := range []string{"/v1/discover?target=localhost", "/v1/discover/", "/v1/%64iscover", "/v1/../discover", "http://localhost/v1/discover", "/v2/discover", "/debug", "/v1/hosts"} {
		if allowedPath(path) {
			t.Errorf("unsafe path allowed %q", path)
		}
	}
}

func TestDiscoveryIncomingRequiresSameOwnerAndSignsIdentity(t *testing.T) {
	calls := 0
	const secret = "discovery-backend-secret"
	backend := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		body, _ := io.ReadAll(r.Body)
		if r.URL.Path != "/v1/discover" || r.Method != "POST" || string(body) != "{}" {
			t.Error("wrong discovery request")
		}
		millis, err := strconv.ParseInt(r.Header.Get("X-Shellfox-Timestamp"), 10, 64)
		if err != nil {
			t.Error(err)
		}
		expected := signedHeaders(secret, peer{ID: "n-peer", Hostname: "my laptop"}, "POST", "/v1/discover", body, time.UnixMilli(millis))
		for name := range expected {
			if expected.Get(name) != r.Header.Get(name) {
				t.Errorf("invalid signature %s", name)
			}
		}
		if r.Header.Get("Authorization") != "" {
			t.Error("caller headers leaked")
		}
		_, _ = io.WriteString(w, `{"ok":true,"value":{"service":"shellfox","protocolVersion":1}}`)
	}))
	defer backend.Close()
	u, _ := url.Parse(backend.URL)
	f := testNetwork()
	b := &bridge{network: f, backend: u, backendSecret: secret}
	call := func() *httptest.ResponseRecorder {
		r := httptest.NewRequest("POST", "/v1/discover", strings.NewReader("{}"))
		r.RemoteAddr = "100.100.1.2:1234"
		r.Header.Set("Content-Type", "application/json")
		r.Header.Set("Authorization", "spoofed")
		r.Header.Set("X-Shellfox-Peer", "spoofed")
		w := httptest.NewRecorder()
		b.incoming(w, r)
		return w
	}
	if w := call(); w.Code != 200 || calls != 1 {
		t.Fatalf("discovery response %d calls %d", w.Code, calls)
	}
	f.who.Node.User = 99
	if w := call(); w.Code != 403 || calls != 1 {
		t.Fatal("discovery bypassed same-owner check")
	}
	f.who.Node.User = 42
	f.who.Node.Tags = []string{"tag:server"}
	if w := call(); w.Code != 403 || calls != 1 {
		t.Fatal("tagged discovery peer accepted")
	}
}

func TestDiscoveryOutgoingUsesFixedPortAndProxyAuthentication(t *testing.T) {
	calls := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		body, _ := io.ReadAll(r.Body)
		if r.URL.Path != "/v1/discover" || r.Method != "POST" || string(body) != "{}" {
			t.Error("wrong request")
		}
		_, _ = io.WriteString(w, `{"ok":true}`)
	}))
	defer server.Close()
	f := testNetwork()
	b := &bridge{network: f, proxySecret: strings.Repeat("p", 32), dial: func(ctx context.Context, network, address string) (net.Conn, error) {
		if network != "tcp" || address != "100.100.1.2:52743" {
			t.Errorf("unsafe target %s %s", network, address)
		}
		return (&net.Dialer{}).DialContext(ctx, "tcp", server.Listener.Addr().String())
	}}
	const envelope = `{"peerId":"n-peer","path":"/v1/discover","body":{}}`
	if w := proxyCall(t, b, envelope, ""); w.Code != 401 || calls != 0 {
		t.Fatal("discovery bypassed proxy token")
	}
	if w := proxyCall(t, b, envelope, "Bearer "+b.proxySecret); w.Code != 200 || calls != 1 {
		t.Fatal("discovery not forwarded")
	}
	f.status.Self.UserID = 99
	if w := proxyCall(t, b, envelope, "Bearer "+b.proxySecret); w.Code != 403 || calls != 1 {
		t.Fatal("foreign discovery forwarded")
	}
}
