package main

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"log"
	"os"
	"os/signal"
	"runtime"
	"strings"
	"sync"
	"time"

	"tailscale.com/logtail"
)

type request struct {
	ID      json.RawMessage `json:"id"`
	Method  string          `json:"method"`
	Payload json.RawMessage `json:"payload,omitempty"`
}

type protocolError struct {
	Code    string `json:"code"`
	Message string `json:"message"`
}

type output struct {
	mu      sync.Mutex
	encoder *json.Encoder
}

func (o *output) send(value interface{}) {
	o.mu.Lock()
	defer o.mu.Unlock()
	_ = o.encoder.Encode(value)
}

func decode(data []byte, target interface{}) error {
	d := json.NewDecoder(bytes.NewReader(data))
	d.DisallowUnknownFields()
	if err := d.Decode(target); err != nil {
		return errors.New("invalid JSON payload")
	}
	if d.Decode(new(interface{})) != io.EOF {
		return errors.New("invalid JSON payload")
	}
	return nil
}

func main() {
	if len(os.Args) >= 2 && os.Args[1] == "--private-permissions" {
		os.Exit(runPrivatePermissions(os.Args[2:], os.Stdout))
	}
	if len(os.Args) >= 2 && os.Args[1] == "--ownership" {
		if len(os.Args) != 3 {
			_ = json.NewEncoder(os.Stdout).Encode(map[string]string{"error": "Invalid ownership arguments"})
			os.Exit(2)
		}
		os.Exit(runOwnership(os.Args[2], os.Stdin, os.Stdout))
	}
	if len(os.Args) == 2 && os.Args[1] == "--probe" {
		_ = json.NewEncoder(os.Stdout).Encode(map[string]interface{}{"protocolVersion": 1, "tailscaleVersion": "1.104.2", "platform": runtime.GOOS, "arch": runtime.GOARCH})
		return
	}
	if len(os.Args) != 1 {
		os.Exit(2)
	}
	// tsnet configuration and credentials come only from the parent protocol.
	for _, entry := range os.Environ() {
		key, _, _ := strings.Cut(entry, "=")
		if strings.HasPrefix(key, "TS_") || strings.HasPrefix(key, "TSNET_") {
			_ = os.Unsetenv(key)
		}
	}
	_ = os.Setenv("TS_NO_LOGS_NO_SUPPORT", "true")
	logtail.Disable()
	log.SetOutput(io.Discard)
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt)
	defer cancel()
	o := &output{encoder: json.NewEncoder(os.Stdout)}
	h := &helper{emit: func(s networkStatus) { o.send(map[string]interface{}{"event": "status", "payload": s}) }}
	defer h.close()
	go func() { <-ctx.Done(); _ = os.Stdin.Close() }()
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 4096), 64*1024)
	for scanner.Scan() {
		var r request
		if decode(scanner.Bytes(), &r) != nil || !validID(r.ID) {
			o.send(map[string]interface{}{"id": nil, "ok": false, "error": protocolError{"invalid-request", "Invalid request"}})
			continue
		}
		callCtx, done := context.WithTimeout(ctx, 30*time.Second)
		value, err := h.call(callCtx, r.Method, r.Payload)
		done()
		if err != nil {
			// Never serialize upstream errors, which can contain keys or login URLs.
			o.send(map[string]interface{}{"id": r.ID, "ok": false, "error": protocolError{"helper-error", err.Error()}})
		} else {
			o.send(map[string]interface{}{"id": r.ID, "ok": true, "value": value})
		}
		if r.Method == "stop" {
			return
		}
	}
}

func validID(id json.RawMessage) bool {
	var v interface{}
	if len(id) == 0 || len(id) > 256 || json.Unmarshal(id, &v) != nil {
		return false
	}
	switch v.(type) {
	case string, float64:
		return true
	}
	return false
}

func (h *helper) call(ctx context.Context, method string, payload json.RawMessage) (interface{}, error) {
	if method == "start" {
		if h.server != nil {
			return nil, errors.New("Helper already started")
		}
		var config startConfig
		if decode(payload, &config) != nil {
			return nil, errors.New("Invalid start configuration")
		}
		return h.start(config)
	}
	if method == "stop" {
		h.close()
		return map[string]bool{"stopped": true}, nil
	}
	if h.client == nil {
		return nil, errors.New("Helper not started")
	}
	switch method {
	case "status":
		return h.status(ctx), nil
	case "peers":
		status, err := h.client.Status(ctx)
		if err != nil {
			return nil, errors.New("Cannot read network peers")
		}
		return map[string]interface{}{"peers": ownPeers(status)}, nil
	case "login":
		var p loginPayload
		if len(payload) != 0 && string(payload) != "null" && decode(payload, &p) != nil {
			return nil, errors.New("Invalid login payload")
		}
		if err := h.health.Exclusive(func() error { return enroll(ctx, h.client, p.AuthKey) }); err != nil {
			return nil, err
		}
		return h.status(ctx), nil
	case "logout":
		if err := h.health.Exclusive(func() error { return h.client.Logout(ctx) }); err != nil {
			return nil, errors.New("Cannot log out")
		}
		return h.status(ctx), nil
	default:
		return nil, errors.New("Unknown method")
	}
}
