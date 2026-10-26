package mobile

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestPollsCannotStarveOperationsAndSaturationNeverQueues(t *testing.T) {
	n := &node{requests: make(chan struct{}, 12), polls: make(chan struct{}, 64)}
	releases := []func(){}
	for range 64 {
		release, err := n.admit("/v1/events")
		if err != nil {
			t.Fatal(err)
		}
		releases = append(releases, release)
	}
	if _, err := n.admit("/v1/events"); err == nil {
		t.Fatal("saturated poll queued")
	}
	for range 12 {
		release, err := n.admit("/v1/request")
		if err != nil {
			t.Fatal("polls starved operation")
		}
		releases = append(releases, release)
	}
	if _, err := n.admit("/v1/request"); err == nil {
		t.Fatal("saturated operation queued")
	}
	for _, release := range releases {
		release()
	}
	if len(n.polls) != 0 || len(n.requests) != 0 {
		t.Fatal("admission slot leaked")
	}
}

func TestStartIsIdempotentOnlyForSameConfiguration(t *testing.T) {
	directory := t.TempDir()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	singleton.node = &node{ctx: ctx, cancel: cancel, directory: filepath.Clean(directory), hostname: "phone"}
	defer func() { singleton.node = nil }()
	if err := Start(directory, "phone"); err != nil {
		t.Fatal(err)
	}
	if err := Start(directory, "another"); err == nil {
		t.Fatal("hostname changed without restart")
	}
	if err := Start(t.TempDir(), "phone"); err == nil {
		t.Fatal("directory changed without restart")
	}
}

func TestDesktopAllowedEndpointsStayCompatible(t *testing.T) {
	data, err := os.ReadFile(filepath.Join("..", "tailscale", "bridge.go"))
	if err != nil {
		t.Fatal(err)
	}
	source := string(data)
	for _, path := range []string{"/v1/pair", "/v1/request", "/v1/events", "/v1/disconnect"} {
		if !allowedPath(path) || !strings.Contains(source, `"`+path+`"`) {
			t.Fatalf("desktop endpoint drift: %s", path)
		}
	}
	for _, path := range []string{"/", "/v1/health", "/v1/request/", "/v1/stream"} {
		if allowedPath(path) {
			t.Fatalf("unexpected endpoint %s", path)
		}
	}
}
