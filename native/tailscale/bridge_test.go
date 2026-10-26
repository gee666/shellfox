package main

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"net/url"
	"strings"
	"testing"
	"time"

	"tailscale.com/client/tailscale/apitype"
	"tailscale.com/ipn/ipnstate"
	"tailscale.com/tailcfg"
	"tailscale.com/types/key"
)

type fakeNetwork struct {
	status *ipnstate.Status
	who    *apitype.WhoIsResponse
	err    error
}

func (f *fakeNetwork) Status(context.Context) (*ipnstate.Status, error) { return f.status, f.err }
func (f *fakeNetwork) WhoIs(context.Context, string) (*apitype.WhoIsResponse, error) {
	return f.who, f.err
}

func testNetwork() *fakeNetwork {
	p := &ipnstate.PeerStatus{ID: "n-peer", HostName: "my laptop", UserID: 42, Online: true,
		TailscaleIPs: []netip.Addr{netip.MustParseAddr("100.100.1.2"), netip.MustParseAddr("fd7a:115c:a1e0::2")}}
	return &fakeNetwork{
		status: &ipnstate.Status{BackendState: "Running", Self: &ipnstate.PeerStatus{ID: "n-self", UserID: 42}, Peer: map[key.NodePublic]*ipnstate.PeerStatus{{}: p}},
		who:    &apitype.WhoIsResponse{Node: &tailcfg.Node{StableID: "n-peer", User: 42}, UserProfile: &tailcfg.UserProfile{ID: 42}},
	}
}

func proxyCall(t *testing.T, b *bridge, envelope string, token string) *httptest.ResponseRecorder {
	t.Helper()
	r := httptest.NewRequest("POST", "/proxy", strings.NewReader(envelope))
	r.Header.Set("Content-Type", "application/json")
	r.Header.Set("Authorization", token)
	w := httptest.NewRecorder()
	b.outgoing(w, r)
	return w
}

func TestOutgoingUsesOnlyKnownPeerFixedPortAndJSONBody(t *testing.T) {
	calls := 0
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		if r.URL.Path != "/v1/request" || r.Method != "POST" {
			t.Error("wrong upstream request")
		}
		body, _ := io.ReadAll(r.Body)
		if string(body) != `{"test":true}` {
			t.Errorf("body = %s", body)
		}
		if r.Header.Get("Authorization") != "" {
			t.Error("proxy token leaked to peer")
		}
		w.WriteHeader(202)
		_, _ = w.Write([]byte(`{"accepted":true}`))
	}))
	defer upstream.Close()
	upstreamURL, _ := url.Parse(upstream.URL)
	b := &bridge{network: testNetwork(), proxySecret: strings.Repeat("p", 32), dial: func(ctx context.Context, network, address string) (net.Conn, error) {
		if network != "tcp" || address != "100.100.1.2:52743" {
			t.Errorf("unsafe target %s %s", network, address)
		}
		return (&net.Dialer{}).DialContext(ctx, "tcp", upstreamURL.Host)
	}}
	w := proxyCall(t, b, `{"peerId":"n-peer","path":"/v1/request","body":{"test":true}}`, "Bearer "+b.proxySecret)
	if w.Code != 202 || w.Body.String() != `{"accepted":true}` || calls != 1 {
		t.Fatalf("response = %d %s, calls=%d", w.Code, w.Body, calls)
	}
}

func TestOutgoingFailsClosed(t *testing.T) {
	b := &bridge{network: testNetwork(), proxySecret: strings.Repeat("p", 32), dial: func(context.Context, string, string) (net.Conn, error) {
		t.Error("unexpected dial")
		return nil, errors.New("not allowed")
	}}
	valid := `{"peerId":"n-peer","path":"/v1/request","body":{}}`
	cases := []struct {
		body, token string
		code        int
	}{
		{valid, "", 401}, {valid, "Bearer wrong", 401},
		{`{"peerId":"127.0.0.1","path":"/v1/request","body":{}}`, "Bearer " + b.proxySecret, 403},
		{`{"peerId":"n-peer","path":"http://localhost/","body":{}}`, "Bearer " + b.proxySecret, 400},
		{`{"peerId":"n-peer","path":"/v1/request?x=1","body":{}}`, "Bearer " + b.proxySecret, 400},
		{`{"peerId":"n-peer","path":"/v1/../request","body":{}}`, "Bearer " + b.proxySecret, 400},
		{`{"peerId":"n-peer","path":"/v1/request","body":{},"target":"localhost"}`, "Bearer " + b.proxySecret, 400},
		{strings.Repeat("x", bodyLimit+1), "Bearer " + b.proxySecret, 413},
	}
	for _, tc := range cases {
		if w := proxyCall(t, b, tc.body, tc.token); w.Code != tc.code {
			t.Errorf("got %d, want %d", w.Code, tc.code)
		}
	}
	f := testNetwork()
	b.network = f
	f.status.Self.UserID = 999
	if w := proxyCall(t, b, valid, "Bearer "+b.proxySecret); w.Code != 403 {
		t.Error("foreign peer accepted")
	}
	f.status.Self.UserID = 42
	for _, p := range f.status.Peer {
		p.Online = false
	}
	if w := proxyCall(t, b, valid, "Bearer "+b.proxySecret); w.Code != 403 {
		t.Error("offline peer accepted")
	}
}

func TestIncomingSignsVerifiedPeerAndStripsSpoofedHeaders(t *testing.T) {
	secret := strings.Repeat("b", 32)
	calls := 0
	backend := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		body, _ := io.ReadAll(r.Body)
		if string(body) != `{"hello":"world"}` {
			t.Errorf("changed body: %s", body)
		}
		if r.Header.Get("Authorization") != "" || r.Header.Get("X-Shellfox-Peer-Id") != "" || r.Header.Get("X-Forwarded-For") != "" {
			t.Error("spoofed headers forwarded")
		}
		var millis int64
		if err := json.Unmarshal([]byte(r.Header.Get("X-Shellfox-Timestamp")), &millis); err != nil {
			t.Error(err)
		}
		expected := signedHeaders(secret, peer{ID: "n-peer", Hostname: "my laptop"}, "POST", "/v1/request", body, time.UnixMilli(millis))
		for name := range expected {
			if expected.Get(name) != r.Header.Get(name) {
				t.Errorf("invalid signature field %s", name)
			}
		}
		w.Header().Set("X-Shellfox-Peer", "spoof")
		w.Header().Set("Set-Cookie", "secret")
		_, _ = w.Write([]byte(`{"ok":true}`))
	}))
	defer backend.Close()
	u, _ := url.Parse(backend.URL)
	b := &bridge{network: testNetwork(), backend: u, backendSecret: secret}
	r := httptest.NewRequest("POST", "/v1/request", strings.NewReader(`{"hello":"world"}`))
	r.RemoteAddr = "100.100.1.2:12345"
	r.Header.Set("Content-Type", "application/json")
	for _, name := range []string{"X-Shellfox-Peer", "X-Shellfox-Peer-Id", "X-Shellfox-Signature", "X-Shellfox-Timestamp", "Authorization", "X-Forwarded-For"} {
		r.Header.Set(name, "spoofed")
	}
	w := httptest.NewRecorder()
	b.incoming(w, r)
	if w.Code != 200 || calls != 1 || w.Header().Get("Set-Cookie") != "" || w.Header().Get("X-Shellfox-Peer") != "" {
		t.Errorf("response=%d calls=%d", w.Code, calls)
	}
}

func TestIncomingRejectsForeignUnknownOrUnverifiedPeer(t *testing.T) {
	cases := []struct {
		name   string
		change func(*fakeNetwork)
	}{
		{"foreign-node", func(f *fakeNetwork) { f.who.Node.User = 999 }},
		{"foreign-profile", func(f *fakeNetwork) { f.who.UserProfile.ID = 999 }},
		{"tagged", func(f *fakeNetwork) { f.who.Node.Tags = []string{"tag:server"} }},
		{"no-user", func(f *fakeNetwork) { f.status.Self.UserID = 0; f.who.Node.User = 0 }},
		{"logged-out", func(f *fakeNetwork) { f.status.BackendState = "NeedsLogin" }},
		{"unknown", func(f *fakeNetwork) { f.who.Node.StableID = "unknown" }},
		{"unverified", func(f *fakeNetwork) { f.who = nil }},
		{"error", func(f *fakeNetwork) { f.err = errors.New("secret-auth-key") }},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			f := testNetwork()
			tc.change(f)
			b := &bridge{network: f}
			if _, err := b.incomingIdentity(context.Background(), "100.100.1.2:42"); err == nil {
				t.Fatal("unauthorized peer accepted")
			}
		})
	}
	b := &bridge{network: testNetwork()}
	for _, remote := range []string{"127.0.0.1:1", "192.168.0.1:1", "100.100.1.3:1", "garbage", "100.100.1.2"} {
		if _, err := b.incomingIdentity(context.Background(), remote); err == nil {
			t.Errorf("invalid remote %s accepted", remote)
		}
	}
	if _, err := b.incomingIdentity(context.Background(), "[fd7a:115c:a1e0::2]:42"); err != nil {
		t.Error(err)
	}
}

func TestSignatureMatchesNodeHMACVector(t *testing.T) {
	headers := signedHeaders(strings.Repeat("b", 32), peer{ID: "n-peer", Hostname: "my laptop"}, "POST", "/v1/request", []byte(`{"hello":"world"}`), time.UnixMilli(1700000000123))
	if headers.Get("X-Shellfox-Signature") != "de4dad52bcc645ae868d2e8975fdb3bae5cd378523e0ea604bf48f87681dc74f" {
		t.Fatal("HMAC differs from Node vector")
	}
	decoded, err := base64.RawURLEncoding.DecodeString(headers.Get("X-Shellfox-Peer"))
	if err != nil || string(decoded) != `{"id":"n-peer","hostname":"my laptop"}` {
		t.Fatalf("peer = %s, %v", decoded, err)
	}
}

func TestForwardDoesNotFollowRedirectOrAcceptOversizedResponse(t *testing.T) {
	for _, large := range []bool{false, true} {
		upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if large {
				_, _ = io.WriteString(w, strings.Repeat("x", responseLimit+1))
				return
			}
			w.Header().Set("Location", "http://127.0.0.1:1/private")
			w.WriteHeader(302)
		}))
		w := httptest.NewRecorder()
		transport := &http.Transport{Proxy: nil}
		forward(w, context.Background(), transport, upstream.URL, []byte(`{}`), nil)
		transport.CloseIdleConnections()
		upstream.Close()
		want := 302
		if large {
			want = 502
		}
		if w.Code != want || w.Header().Get("Location") != "" {
			t.Errorf("code = %d want %d", w.Code, want)
		}
	}
}
