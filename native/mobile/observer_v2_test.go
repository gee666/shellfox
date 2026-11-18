package mobile

import (
	"context"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
)

var observerV2Paths = []string{"/v2/discover", "/v2/pair", "/v2/request", "/v2/events", "/v2/disconnect"}

func TestObserverV2AllowlistIsExactlyAdditiveAndMatchesDesktop(t *testing.T) {
	data, err := os.ReadFile(filepath.Join("..", "tailscale", "bridge.go"))
	if err != nil {
		t.Fatal(err)
	}
	for _, version := range []string{"v1", "v2"} {
		for _, action := range []string{"discover", "pair", "request", "events", "disconnect"} {
			path := "/" + version + "/" + action
			if !allowedPath(path) || !strings.Contains(string(data), `"`+path+`"`) {
				t.Fatalf("endpoint drift: %s", path)
			}
		}
	}
	for _, path := range []string{"/v2", "/v2/", "/v2/health", "/v2/stream", "/v2/request/", "/v2/request?query=1", "/v2/events#fragment", "/v2/%72equest", "/v2/../v1/request", "/V2/request", "/v20/request", "http://127.0.0.1/v2/request", "//other/v2/request"} {
		if allowedPath(path) || validateRequest("n-peer", path, "{}") == nil {
			t.Fatalf("unsafe endpoint: %s", path)
		}
	}
}
func TestObserverV2UsesSamePollAndOperationPoolsWithoutIncreasingBudgets(t *testing.T) {
	n := &node{requests: make(chan struct{}, 12), polls: make(chan struct{}, 64)}
	releases := []func(){}
	defer func() {
		for _, release := range releases {
			release()
		}
	}()
	for i := 0; i < 64; i++ {
		path := []string{"/v1/events", "/v2/events"}[i%2]
		release, err := n.admit(path)
		if err != nil {
			t.Fatal(err)
		}
		releases = append(releases, release)
	}
	for _, path := range []string{"/v1/events", "/v2/events"} {
		if _, err := n.admit(path); err == nil {
			t.Fatal("poll capacity grew with profile count")
		}
	}
	for i := 0; i < 12; i++ {
		path := []string{"/v1/request", "/v2/request"}[i%2]
		release, err := n.admit(path)
		if err != nil {
			t.Fatal("polls starved operations")
		}
		releases = append(releases, release)
	}
	for _, path := range []string{"/v1/request", "/v2/request"} {
		if _, err := n.admit(path); err == nil {
			t.Fatal("operation capacity grew with profile count")
		}
	}
	for _, path := range []string{"/v1/events/", "/v2/events?x=1", "/v20/events", "/v2/request"} {
		if isEventPath(path) {
			t.Fatal("non-exact poll classification")
		}
	}
}
func TestObserverV2EveryRouteIsOnePostWithNoCallerIdentityOrFallback(t *testing.T) {
	for _, path := range observerV2Paths {
		t.Run(path, func(t *testing.T) {
			var calls atomic.Int32
			const body = `{"clientId":"fixture", "value":true}`
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				calls.Add(1)
				data, _ := io.ReadAll(r.Body)
				if r.Method != "POST" || r.URL.Path != path || string(data) != body || r.Header.Get("Content-Type") != "application/json" {
					t.Error("path/method/body changed")
				}
				for _, name := range []string{"Authorization", "Cookie", "X-Shellfox-Peer", "X-Shellfox-Signature", "X-Forwarded-For", "Idempotency-Key"} {
					if r.Header.Get(name) != "" {
						t.Error("caller identity/header escaped")
					}
				}
				w.WriteHeader(404)
				_, _ = io.WriteString(w, "missing route, not a schema hint")
			}))
			defer server.Close()
			result, err := statusRequest(context.Background(), testNetwork(), localStatusDial(server), path, body)
			if err != nil || result == nil || result.StatusCode() != 404 || result.Body() != "" || calls.Load() != 1 {
				t.Fatalf("status or single-attempt contract changed: %v", err)
			}
		})
	}
}
func TestObserverV2LegacyRequestKeepsSuccessAndGenericRejection(t *testing.T) {
	for _, code := range []int{200, 404, 403} {
		server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			w.WriteHeader(code)
			_, _ = io.WriteString(w, `{"ok":false,"error":{"code":"ACCESS_DENIED","retryable":false}}`)
		}))
		pool := &transportPool{}
		body, err := pool.request(context.Background(), testNetwork(), localStatusDial(server), "n-peer", "/v1/request", "{}")
		pool.close()
		server.Close()
		if code == 200 {
			if err != nil || !strings.Contains(body, "ACCESS_DENIED") {
				t.Fatal("application denial reclassified")
			}
		} else if err == nil || body != "" || err.Error() != "Remote host rejected request" {
			t.Fatal("legacy rejection semantics changed")
		}
	}
}
