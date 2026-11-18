package mobile

import (
	"context"
	"errors"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

var _ func(string, string, string) (*RequestResult, error) = RequestWithStatus
var _ func(string, string, string) (string, error) = Request

func statusRequest(ctx context.Context, client peerClient, dial dialFunc, path, body string) (*RequestResult, error) {
	pool := &transportPool{}
	defer pool.close()
	return pool.requestWithStatus(ctx, client, dial, "n-peer", path, body)
}
func localStatusDial(server *httptest.Server) dialFunc {
	return func(ctx context.Context, network, target string) (net.Conn, error) {
		if network != "tcp" || target != "100.100.1.2:52743" {
			return nil, errors.New("unexpected test target")
		}
		return (&net.Dialer{}).DialContext(ctx, "tcp", server.Listener.Addr().String())
	}
}
func TestRequestWithStatusHasReadOnlyGomobileGetters(t *testing.T) {
	typ := reflect.TypeFor[RequestResult]()
	for i := 0; i < typ.NumField(); i++ {
		if typ.Field(i).IsExported() {
			t.Fatal("mutable field exported to gomobile")
		}
	}
	methods := reflect.TypeFor[*RequestResult]()
	if methods.NumMethod() != 2 {
		t.Fatal("unexpected result mutation API")
	}
	for _, name := range []string{"StatusCode", "Body"} {
		if _, ok := methods.MethodByName(name); !ok {
			t.Fatalf("missing getter %s", name)
		}
	}
}
func TestRequestWithStatusPreservesActualStatusWithoutFallbackOrDiagnosticBody(t *testing.T) {
	for _, code := range []int{200, 202, 302, 307, 401, 403, 404, 429, 500, 503} {
		t.Run(http.StatusText(code), func(t *testing.T) {
			var calls atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				calls.Add(1)
				if r.Method != "POST" || r.URL.Path != "/v2/discover" || r.Header.Get("Content-Type") != "application/json" {
					t.Error("request changed")
				}
				w.Header().Set("Location", "http://127.0.0.1:1/not-followed")
				w.WriteHeader(code)
				if code >= 200 && code < 300 {
					_, _ = io.WriteString(w, " {\"ok\":true} \n")
				} else {
					_, _ = io.WriteString(w, "private upstream diagnostics mentioning 404")
				}
			}))
			defer server.Close()
			response, err := statusRequest(context.Background(), testNetwork(), localStatusDial(server), "/v2/discover", "{}")
			if err != nil || response == nil {
				t.Fatalf("missing actual status: %v", err)
			}
			if response.StatusCode() != code || calls.Load() != 1 {
				t.Fatal("status changed or request replayed")
			}
			if code >= 200 && code < 300 {
				if response.Body() != " {\"ok\":true} \n" {
					t.Fatal("JSON bytes changed")
				}
			} else if response.Body() != "" {
				t.Fatal("non-2xx diagnostics exposed")
			}
			legacy, legacyErr := legacyResponse(response, nil)
			if code >= 200 && code < 300 {
				if legacyErr != nil || legacy != response.Body() {
					t.Fatal("legacy success changed")
				}
			} else if legacy != "" || legacyErr == nil || legacyErr.Error() != "Remote host rejected request" {
				t.Fatal("legacy rejection changed")
			}
		})
	}
}
func TestRequestWithStatusErrorsNeverBecomeSyntheticHTTP404(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	broken := testNetwork()
	broken.err = errors.New("404 private network failure")
	foreign := testNetwork()
	foreign.status.Self.UserID = 99
	for _, tc := range []struct {
		name       string
		ctx        context.Context
		client     peerClient
		path, body string
	}{
		{"network", context.Background(), broken, "/v2/discover", "{}"},
		{"ownership", context.Background(), foreign, "/v2/discover", "{}"},
		{"canceled", ctx, testNetwork(), "/v2/discover", "{}"},
		{"path", context.Background(), testNetwork(), "/v2/discover?query=404", "{}"},
		{"json", context.Background(), testNetwork(), "/v2/discover", "{"},
		{"request limit", context.Background(), testNetwork(), "/v2/discover", `"` + strings.Repeat("x", bodyLimit) + `"`},
	} {
		t.Run(tc.name, func(t *testing.T) {
			dial := func(context.Context, string, string) (net.Conn, error) {
				return nil, errors.New("404 fake dial failure")
			}
			result, err := statusRequest(tc.ctx, tc.client, dial, tc.path, tc.body)
			if err == nil || result != nil || strings.Contains(err.Error(), "404") {
				t.Fatal("transport error became an HTTP status or leaked diagnostics")
			}
		})
	}
	for _, mode := range []string{"bad json", "large", "compressed", "truncated", "deadline"} {
		t.Run(mode, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				switch mode {
				case "bad json":
					_, _ = io.WriteString(w, "404 private non-json")
				case "large":
					_, _ = io.WriteString(w, `"`+strings.Repeat("x", responseLimit)+`"`)
				case "compressed":
					w.Header().Set("Content-Encoding", "gzip")
					_, _ = w.Write([]byte{0x1f, 0x8b})
				case "truncated":
					w.Header().Set("Content-Length", "20")
					_, _ = io.WriteString(w, "{}")
				case "deadline":
					_, _ = io.Copy(io.Discard, r.Body)
					<-r.Context().Done()
				}
			}))
			defer server.Close()
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			if mode == "deadline" {
				cancel()
				ctx, cancel = context.WithTimeout(context.Background(), 20*time.Millisecond)
			}
			defer cancel()
			result, err := statusRequest(ctx, testNetwork(), localStatusDial(server), "/v2/events", "{}")
			if result != nil || err == nil {
				t.Fatal("invalid or timed out response became success/status")
			}
		})
	}
}
