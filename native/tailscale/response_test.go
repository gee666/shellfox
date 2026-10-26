package main

import (
	"context"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestForwardAcceptsLargeReplayAndEventResponses(t *testing.T) {
	// A 256 KiB terminal snapshot can expand sixfold when JSON escapes control
	// characters. Event batches are larger still. Request limits do not apply.
	for _, size := range []int{6 * 256 * 1024, 2 * 1024 * 1024, responseLimit} {
		upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			_, _ = io.WriteString(w, strings.Repeat("x", size))
		}))
		w := httptest.NewRecorder()
		transport := &http.Transport{Proxy: nil}
		forward(w, context.Background(), transport, upstream.URL, []byte(`{}`), nil)
		transport.CloseIdleConnections()
		upstream.Close()
		if w.Code != 200 || w.Body.Len() != size {
			t.Errorf("response %d: code=%d bytes=%d", size, w.Code, w.Body.Len())
		}
	}
}
