package mobile

import (
	"context"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func TestExactByteLimitsRemainOneMiBAndEightMiB(t *testing.T) {
	if bodyLimit != 1<<20 || responseLimit != 8<<20 || requestTimeout != 30*time.Second {
		t.Fatal("native transport limits changed")
	}
	requestBody := `"` + strings.Repeat("x", bodyLimit-2) + `"`
	responseBody := `"` + strings.Repeat("y", responseLimit-2) + `"`
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		body, _ := io.ReadAll(r.Body)
		if len(body) != bodyLimit || string(body) != requestBody || r.URL.Path != "/v1/request" {
			t.Error("request boundary changed")
		}
		_, _ = io.WriteString(w, responseBody)
	}))
	defer server.Close()
	result, err := statusRequest(context.Background(), testNetwork(), localStatusDial(server), "/v1/request", requestBody)
	if err != nil || result == nil || len(result.Body()) != responseLimit || result.Body() != responseBody || calls.Load() != 1 {
		t.Fatalf("exact boundary rejected: %v", err)
	}
}
func TestUncertainPostIsNeverReplayedOrReportedAsHTTP404(t *testing.T) {
	for _, path := range []string{"/v1/pair", "/v1/request", "/v1/events", "/v1/disconnect"} {
		t.Run(path, func(t *testing.T) {
			var calls atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				_, _ = io.Copy(io.Discard, r.Body)
				if r.URL.Path == "/v1/discover" {
					_, _ = io.WriteString(w, "{}")
					return
				}
				calls.Add(1)
				connection, _, err := w.(http.Hijacker).Hijack()
				if err != nil {
					t.Error(err)
					return
				}
				_ = connection.Close()
			}))
			defer server.Close()
			pool := &transportPool{}
			defer pool.close()
			dial := localStatusDial(server)
			if _, err := pool.requestWithStatus(context.Background(), testNetwork(), dial, "n-peer", "/v1/discover", "{}"); err != nil {
				t.Fatal(err)
			}
			result, err := pool.requestWithStatus(context.Background(), testNetwork(), dial, "n-peer", path, "{}")
			if err == nil || result != nil || calls.Load() != 1 {
				t.Fatal("uncertain POST replayed or became a response status")
			}
		})
	}
}
func TestExportedAPIErrorsHaveNoResultOrFallbackSignal(t *testing.T) {
	if singleton.node != nil {
		t.Fatal("test must not start a native node")
	}
	for _, path := range []string{"/v1/discover", "/v1/discover", "http://other/v1/discover", "/v1/events/"} {
		result, err := RequestWithStatus("n-peer", path, "{}")
		if result != nil || err == nil {
			t.Fatal("stopped/invalid request produced status")
		}
		body, legacyErr := Request("n-peer", path, "{}")
		if body != "" || legacyErr == nil || legacyErr.Error() != err.Error() {
			t.Fatal("legacy error changed")
		}
	}
}
