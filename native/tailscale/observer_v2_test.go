package main

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

var observerV2Paths = []string{"/v2/discover", "/v2/pair", "/v2/request", "/v2/events", "/v2/disconnect"}

func TestObserverV2ExactAllowlistDoesNotExpandV1OrCreateAliases(t *testing.T) {
	for _, version := range []string{"v1", "v2"} {
		for _, action := range []string{"discover", "pair", "request", "events", "disconnect"} {
			if !allowedPath("/" + version + "/" + action) {
				t.Fatal("approved path unavailable")
			}
		}
	}
	for _, path := range []string{"/v2", "/v2/", "/v2/health", "/v2/stream", "/v2/request/", "/v2/request?x=1", "/v2/events#fragment", "/v2/%72equest", "/v2/../v1/request", "/V2/request", "/v20/request", "http://127.0.0.1/v2/request", "//other/v2/request"} {
		if allowedPath(path) {
			t.Fatalf("unexpected path %s", path)
		}
	}
}
func TestObserverV2IncomingUsesActualWhoIsAndBindsExactPathBody(t *testing.T) {
	for _, path := range observerV2Paths {
		t.Run(path, func(t *testing.T) {
			secret := strings.Repeat("b", 32)
			const body = " {\"clientId\":\"fixture\", \"peerId\":\"spoof\"} \n"
			var calls atomic.Int32
			backend := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				calls.Add(1)
				data, _ := io.ReadAll(r.Body)
				if r.URL.Path != path || r.Method != "POST" || string(data) != body {
					t.Error("method/path/body rewritten")
				}
				for _, name := range []string{"Authorization", "Cookie", "X-Forwarded-For", "X-Shellfox-Peer-Id", "Idempotency-Key"} {
					if r.Header.Get(name) != "" {
						t.Error("spoofed header forwarded")
					}
				}
				peerData, err := base64.RawURLEncoding.DecodeString(r.Header.Get("X-Shellfox-Peer"))
				if err != nil {
					t.Fatal(err)
				}
				if string(peerData) != `{"id":"n-peer","hostname":"my laptop"}` {
					t.Error("body/header identity trusted instead of WhoIs")
				}
				stamp := r.Header.Get("X-Shellfox-Timestamp")
				digest := sha256.Sum256(data)
				mac := hmac.New(sha256.New, []byte(secret))
				_, _ = io.WriteString(mac, stamp+"\n"+r.Header.Get("X-Shellfox-Peer")+"\nPOST\n"+path+"\n"+hex.EncodeToString(digest[:]))
				if r.Header.Get("X-Shellfox-Signature") != hex.EncodeToString(mac.Sum(nil)) {
					t.Error("exact v2 path/body HMAC mismatch")
				}
				wrong := signedHeaders(secret, peer{ID: "n-peer", Hostname: "my laptop"}, "POST", strings.Replace(path, "/v2/", "/v1/", 1), data, time.UnixMilli(mustTimestamp(t, stamp)))
				if wrong.Get("X-Shellfox-Signature") == r.Header.Get("X-Shellfox-Signature") {
					t.Error("v1 signature authorized v2")
				}
				w.Header().Set("Set-Cookie", "private")
				_, _ = io.WriteString(w, `{"ok":true}`)
			}))
			defer backend.Close()
			u, _ := url.Parse(backend.URL)
			network := testNetwork()
			b := &bridge{network: network, backend: u, backendSecret: secret}
			call := func(method, target string) *httptest.ResponseRecorder {
				r := httptest.NewRequest(method, target, strings.NewReader(body))
				r.RemoteAddr = "100.100.1.2:12345"
				r.Header.Set("Content-Type", "application/json")
				for _, name := range []string{"Authorization", "Cookie", "X-Forwarded-For", "X-Shellfox-Peer", "X-Shellfox-Signature", "X-Shellfox-Peer-Id", "Idempotency-Key"} {
					r.Header.Set(name, "spoof")
				}
				w := httptest.NewRecorder()
				b.incoming(w, r)
				return w
			}
			if w := call("POST", path); w.Code != 200 || calls.Load() != 1 || w.Header().Get("Set-Cookie") != "" {
				t.Fatal("v2 incoming failed or leaked response headers")
			}
			network.who.Node.User = 99
			if w := call("POST", path); w.Code != 403 || calls.Load() != 1 {
				t.Fatal("v2 bypassed same-owner WhoIs proof")
			}
			network.who.Node.User = 42
			if w := call("GET", path); w.Code != 405 || calls.Load() != 1 {
				t.Fatal("non-POST v2 forwarded")
			}
			for _, alias := range []string{path + "?x=1", path + "/", strings.Replace(path, "/v2/", "/v%32/", 1)} {
				if w := call("POST", alias); w.Code < 400 || calls.Load() != 1 {
					t.Fatal("v2 alias forwarded")
				}
			}
		})
	}
}
func mustTimestamp(t *testing.T, raw string) int64 {
	t.Helper()
	var value int64
	if json.Unmarshal([]byte(raw), &value) != nil {
		t.Fatal("invalid signed timestamp")
	}
	return value
}
func TestObserverV2ProxyPreservesActual404AndNeverInfersFallbackOrCallerIdentity(t *testing.T) {
	for _, path := range observerV2Paths {
		t.Run(path, func(t *testing.T) {
			var calls atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				calls.Add(1)
				if r.URL.Path != path || r.Method != "POST" {
					t.Error("route changed")
				}
				if r.Header.Get("Authorization") != "" || r.Header.Get("X-Shellfox-Peer") != "" {
					t.Error("loopback token leaked")
				}
				w.Header().Set("Location", "http://127.0.0.1:1/not-followed")
				w.WriteHeader(404)
				_, _ = io.WriteString(w, "diagnostics are not fallback authority")
			}))
			defer server.Close()
			b := &bridge{network: testNetwork(), proxySecret: strings.Repeat("p", 32), dial: func(ctx context.Context, network, target string) (net.Conn, error) {
				if network != "tcp" || target != "100.100.1.2:52743" {
					t.Error("arbitrary tailnet target")
				}
				return (&net.Dialer{}).DialContext(ctx, "tcp", server.Listener.Addr().String())
			}}
			envelope := `{"peerId":"n-peer","path":"` + path + `","body":{}}`
			if w := proxyCall(t, b, envelope, ""); w.Code != 401 || calls.Load() != 0 {
				t.Fatal("v2 bypassed proxy bearer")
			}
			if w := proxyCall(t, b, envelope, "Bearer "+b.proxySecret); w.Code != 404 || calls.Load() != 1 || w.Header().Get("Location") != "" {
				t.Fatal("HTTP status hidden, redirect leaked or request repeated")
			}
		})
	}
}
