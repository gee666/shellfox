package main

import (
	"bytes"
	"path/filepath"
	"strings"
	"testing"
)

func TestPrivatePermissionsRejectInvalidArgumentsWithoutEcho(t *testing.T) {
	path := filepath.Join(t.TempDir(), "private")
	for _, args := range [][]string{
		nil, {path}, {path, "file"}, {path, "file", "check", "extra"},
		{"relative", "file", "check"}, {path, "other", "check"},
		{path, "file", "other"}, {path + "\x00", "file", "check"},
		{path + "\nsecret-marker", "file", "check"},
		{strings.Repeat("x", 32768), "file", "check"},
	} {
		var output bytes.Buffer
		if code := runPrivatePermissions(args, &output); code != 2 {
			t.Fatalf("invalid arguments returned %d", code)
		}
		if output.String() != "{\"error\":\"Invalid private permissions arguments\"}\n" {
			t.Fatal("invalid command leaked input or produced unexpected output")
		}
	}
}

func TestPrivatePermissionsMissingPathFailsClosed(t *testing.T) {
	var output bytes.Buffer
	code := runPrivatePermissions([]string{filepath.Join(t.TempDir(), "missing"), "file", "check"}, &output)
	if code != 1 || output.String() != "{\"error\":\"Unsafe private permissions\"}\n" {
		t.Fatal("missing or unsupported target did not fail closed")
	}
}
