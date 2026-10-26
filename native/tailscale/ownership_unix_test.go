//go:build darwin || linux

package main

import (
	"os"
	"path/filepath"
	"testing"

	"golang.org/x/sys/unix"
)

func TestOwnershipUnixPermissionsDoNotChangeAppdataRoot(t *testing.T) {
	dir := t.TempDir()
	if err := os.Chmod(dir, 0750); err != nil {
		t.Fatal(err)
	}
	private := filepath.Join(dir, ownershipDirectory)
	if err := os.Mkdir(private, 0755); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(private, ownershipFilename)
	if err := os.WriteFile(path, []byte("retained"), 0644); err != nil {
		t.Fatal(err)
	}
	file, locked, err := acquireOwnership(dir)
	if err != nil || !locked {
		t.Fatalf("ownership: %v", err)
	}
	defer file.Close()
	for path, want := range map[string]os.FileMode{dir: 0750, private: 0700, path: 0600} {
		info, err := os.Stat(path)
		if err != nil || info.Mode().Perm() != want {
			t.Errorf("incorrect permissions on %s", filepath.Base(path))
		}
	}
	_ = file.Close()
	data, err := os.ReadFile(path)
	if err != nil || string(data) != "retained" {
		t.Fatal("ownership truncated the stable lock file")
	}
}

func TestOwnershipUnixRejectsFIFOWithoutBlocking(t *testing.T) {
	dir := t.TempDir()
	if err := os.Mkdir(filepath.Join(dir, ownershipDirectory), 0700); err != nil {
		t.Fatal(err)
	}
	if err := unix.Mkfifo(filepath.Join(dir, ownershipDirectory, ownershipFilename), 0600); err != nil {
		t.Fatal(err)
	}
	if file, locked, err := acquireOwnership(dir); err == nil || locked || file != nil {
		t.Fatal("FIFO accepted as ownership lock")
	}
}
