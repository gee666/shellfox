//go:build darwin || linux

package main

import (
	"os"
	"path/filepath"
	"testing"

	"golang.org/x/sys/unix"
)

func TestOwnershipUnixRejectsUnsafeExistingPermissionsWithoutRepair(t *testing.T) {
	for _, unsafe := range []string{"root", "directory", "file"} {
		t.Run(unsafe, func(t *testing.T) {
			dir := ownershipTestRoot(t)
			private := filepath.Join(dir, ownershipDirectory)
			if err := os.Mkdir(private, 0700); err != nil {
				t.Fatal(err)
			}
			path := filepath.Join(private, ownershipFilename)
			if err := os.WriteFile(path, []byte("retained"), 0600); err != nil {
				t.Fatal(err)
			}
			modes := map[string]os.FileMode{dir: 0700, private: 0700, path: 0600}
			target := map[string]string{"root": dir, "directory": private, "file": path}[unsafe]
			modes[target] = map[string]os.FileMode{"root": 0770, "directory": 0755, "file": 0644}[unsafe]
			if err := os.Chmod(target, modes[target]); err != nil {
				t.Fatal(err)
			}
			file, locked, err := acquireOwnership(dir)
			if file != nil {
				t.Cleanup(func() { _ = file.Close() })
			}
			if err == nil || locked || file != nil {
				t.Fatal("unsafe existing permissions were adopted")
			}
			for name, want := range modes {
				info, err := os.Stat(name)
				if err != nil || info.Mode().Perm() != want {
					t.Fatal("ownership repaired unsafe permissions")
				}
			}
			data, err := os.ReadFile(path)
			if err != nil || string(data) != "retained" {
				t.Fatal("ownership changed the stable lock file")
			}
		})
	}
}

func TestOwnershipUnixReadOnlyGroupRootIsNotRepaired(t *testing.T) {
	dir := ownershipTestRoot(t)
	if err := os.Chmod(dir, 0750); err != nil {
		t.Fatal(err)
	}
	file, locked, err := acquireOwnership(dir)
	if err != nil || !locked {
		t.Fatalf("ownership: %v", err)
	}
	defer file.Close()
	info, err := os.Stat(dir)
	if err != nil || info.Mode().Perm() != 0750 {
		t.Fatal("root permissions changed")
	}
}

func TestOwnershipUnixRejectsFIFOWithoutBlocking(t *testing.T) {
	dir := ownershipTestRoot(t)
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
