package main

import (
	"os"
	"path/filepath"
	"testing"
)

func TestControlURLDefaultIsUnchanged(t *testing.T) {
	got, err := controlURL("")
	if err != nil || got != "https://controlplane.tailscale.com" {
		t.Fatal("default control changed")
	}
}
func TestControlURLInvalidStartHasNoStateOrNodeEffects(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "not-created")
	h := &helper{}
	_, err := h.start(startConfig{StateDirectory: directory, ControlURL: "http://192.0.2.1:1234"})
	if err == nil || h.server != nil || h.client != nil {
		t.Fatal("invalid control origin reached node creation")
	}
	if _, err := os.Lstat(directory); !os.IsNotExist(err) {
		t.Fatal("invalid control origin changed state directory")
	}
}
