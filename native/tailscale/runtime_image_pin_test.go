package main

import (
	"crypto/sha256"
	"encoding/hex"
	"os"
	"path/filepath"
	"testing"
)

func TestRuntimePinArgumentsPreserveLegacyModes(t *testing.T) {
	pin := sha256.Sum256([]byte("trusted package test bytes"))
	text := hex.EncodeToString(pin[:])
	expected, network, err := runtimeNetworkArguments([]string{"--expected-sha256", text})
	if err != nil || !network || string(expected) != string(pin[:]) {
		t.Fatal("external pin argument failed")
	}
	if expected, network, err = runtimeNetworkArguments(nil); err != nil || !network || expected != nil {
		t.Fatal("legacy normal mode changed")
	}
	for _, args := range [][]string{{"--probe"}, {"--ownership", "directory", "identity"}, {"--private-permissions", "verify", "directory"}} {
		if expected, network, err = runtimeNetworkArguments(args); err != nil || network || expected != nil {
			t.Fatal("existing offline mode changed")
		}
	}
	for _, args := range [][]string{{"--expected-sha256"}, {"--expected-sha256", "bad"}, {"--expected-sha256", text, "extra"}, {"--expected-sha256", string(make([]byte, 64))}} {
		if _, _, err := runtimeNetworkArguments(args); err == nil {
			t.Fatal("invalid expected pin accepted")
		}
	}
}

func TestRuntimePinChecksSameDescriptorAgainstExternalDigest(t *testing.T) {
	content := []byte("external trusted package fixture bytes")
	expected := sha256.Sum256(content)
	path := filepath.Join(t.TempDir(), "candidate.exe")
	if err := os.WriteFile(path, content, 0600); err != nil {
		t.Fatal(err)
	}
	file, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	if err := verifyRuntimeImagePin(file, expected[:]); err != nil {
		t.Fatal("matching external pin rejected")
	}
	if err := verifyRuntimeImagePin(file, make([]byte, 32)); err == nil {
		t.Fatal("wrong external pin accepted")
	}
	if err := verifyRuntimeImagePin(nil, expected[:]); err == nil {
		t.Fatal("missing descriptor accepted")
	}
	if err := verifyRuntimeImagePin(file, expected[:31]); err == nil {
		t.Fatal("invalid digest length accepted")
	}
}
