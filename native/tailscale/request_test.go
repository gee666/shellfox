package main

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"testing"
)

func requestBody(size int) []byte {
	return []byte(`{"pad":"` + strings.Repeat("x", size-len(`{"pad":""}`)) + `"}`)
}

func TestIncomingRequestLimitExactBoundary(t *testing.T) {
	calls := 0
	backend := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		body, _ := io.ReadAll(r.Body)
		if len(body) != bodyLimit {
			t.Errorf("body length %d", len(body))
		}
		_, _ = w.Write([]byte(`{}`))
	}))
	defer backend.Close()
	u, _ := url.Parse(backend.URL)
	b := &bridge{network: testNetwork(), backend: u, backendSecret: strings.Repeat("b", 32)}
	for _, size := range []int{bodyLimit, bodyLimit + 1} {
		r := httptest.NewRequest("POST", "/v1/request", bytes.NewReader(requestBody(size)))
		r.RemoteAddr = "100.100.1.2:12345"
		r.Header.Set("Content-Type", "application/json")
		w := httptest.NewRecorder()
		b.incoming(w, r)
		want := 200
		if size > bodyLimit {
			want = 413
		}
		if w.Code != want {
			t.Errorf("size %d: got %d want %d", size, w.Code, want)
		}
	}
	if calls != 1 {
		t.Errorf("backend calls=%d", calls)
	}
}

func TestOutgoingRequestBoundaryAndEscaped64KiBInput(t *testing.T) {
	const path = "/v1/request"
	escapedBody, err := json.Marshal(map[string]interface{}{"input": map[string]string{"data": strings.Repeat("\x1b", 64*1024)}})
	if err != nil {
		t.Fatal(err)
	}
	escapedEnvelope, _ := json.Marshal(proxyRequest{PeerID: "n-peer", Path: path, Body: escapedBody})
	baseEnvelope, _ := json.Marshal(proxyRequest{PeerID: "n-peer", Path: path, Body: json.RawMessage(`{"pad":""}`)})
	paddingSize := bodyLimit - len(baseEnvelope) + len(`{"pad":""}`)
	boundaryBody := requestBody(paddingSize)
	boundaryEnvelope, _ := json.Marshal(proxyRequest{PeerID: "n-peer", Path: path, Body: boundaryBody})
	overEnvelope, _ := json.Marshal(proxyRequest{PeerID: "n-peer", Path: path, Body: requestBody(paddingSize + 1)})
	if len(boundaryEnvelope) != bodyLimit || len(overEnvelope) != bodyLimit+1 {
		t.Fatal("incorrect boundary fixture")
	}
	if len(escapedBody) <= 256*1024 {
		t.Fatal("escaped fixture does not exercise expansion")
	}
	var expected []byte
	calls := 0
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		body, _ := io.ReadAll(r.Body)
		if !bytes.Equal(body, expected) {
			t.Error("request body changed or truncated")
		}
		_, _ = w.Write([]byte(`{}`))
	}))
	defer upstream.Close()
	u, _ := url.Parse(upstream.URL)
	b := &bridge{network: testNetwork(), proxySecret: strings.Repeat("p", 32), dial: func(ctx context.Context, network, address string) (net.Conn, error) {
		if address != "100.100.1.2:52743" {
			t.Errorf("unexpected target %s", address)
		}
		return (&net.Dialer{}).DialContext(ctx, network, u.Host)
	}}
	for _, tc := range []struct {
		envelope, body []byte
		code           int
	}{
		{escapedEnvelope, escapedBody, 200}, {boundaryEnvelope, boundaryBody, 200}, {overEnvelope, nil, 413},
	} {
		expected = tc.body
		w := proxyCall(t, b, string(tc.envelope), "Bearer "+b.proxySecret)
		if w.Code != tc.code {
			t.Errorf("response %d: %s", w.Code, w.Body)
		}
	}
	if calls != 2 {
		t.Errorf("upstream calls=%d", calls)
	}
}
