package main

import (
	"bytes"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestOwnershipExclusiveAndStableAcrossReopen(t *testing.T) {
	dir := t.TempDir()
	file, locked, err := acquireOwnership(dir)
	if err != nil || !locked {
		t.Fatalf("first ownership: %v", err)
	}
	defer file.Close()
	path := filepath.Join(dir, ownershipDirectory, ownershipFilename)
	before, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	for range 2 {
		other, locked, err := acquireOwnership(dir)
		if err != nil || locked || other != nil {
			t.Fatalf("duplicate ownership: locked=%v err=%v", locked, err)
		}
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
	next, locked, err := acquireOwnership(dir)
	if err != nil || !locked {
		t.Fatalf("ownership after close: %v", err)
	}
	defer next.Close()
	after, err := os.Stat(path)
	if err != nil || !os.SameFile(before, after) {
		t.Fatal("lock path was removed or replaced")
	}
}

func TestOwnershipEOFReleasesWithoutRemovingFile(t *testing.T) {
	dir := t.TempDir()
	input, parent := io.Pipe()
	defer input.Close()
	defer parent.Close()
	output, reply := io.Pipe()
	defer output.Close()
	defer reply.Close()
	done := make(chan int, 1)
	go func() { done <- runOwnership(dir, input, reply) }()
	line := make([]byte, len("{\"locked\":true}\n"))
	if _, err := io.ReadFull(output, line); err != nil || string(line) != "{\"locked\":true}\n" {
		t.Fatal("invalid ownership acknowledgement")
	}
	if _, locked, err := acquireOwnership(dir); err != nil || locked {
		t.Fatal("ownership released before EOF")
	}
	if err := parent.Close(); err != nil {
		t.Fatal(err)
	}
	select {
	case code := <-done:
		if code != 0 {
			t.Error("ownership EOF failed")
		}
	case <-time.After(5 * time.Second):
		t.Fatal("ownership did not stop at EOF")
	}
	file, locked, err := acquireOwnership(dir)
	if err != nil || !locked {
		t.Fatalf("EOF did not release ownership: %v", err)
	}
	_ = file.Close()
}

func TestOwnershipRejectsInvalidPathsWithoutLeakingThem(t *testing.T) {
	dir := t.TempDir()
	regular := filepath.Join(dir, "not-a-directory-DO-NOT-ECHO")
	if err := os.WriteFile(regular, nil, 0600); err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{"relative", "", regular, filepath.Join(dir, "missing-DO-NOT-ECHO")} {
		var output bytes.Buffer
		if runOwnership(path, strings.NewReader(""), &output) == 0 {
			t.Error("invalid ownership path accepted")
		}
		if output.String() != "{\"error\":\"Cannot acquire host ownership\"}\n" {
			t.Error("ownership error is not constant")
		}
	}
	for _, target := range []string{ownershipDirectory, filepath.Join(ownershipDirectory, ownershipFilename)} {
		root := t.TempDir()
		path := filepath.Join(root, target)
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			t.Fatal(err)
		}
		if target == ownershipDirectory {
			if err := os.WriteFile(path, nil, 0600); err != nil {
				t.Fatal(err)
			}
		} else if err := os.Mkdir(path, 0700); err != nil {
			t.Fatal(err)
		}
		if file, locked, err := acquireOwnership(root); err == nil || locked || file != nil {
			t.Error("invalid lock file type accepted")
		}
	}
}

func TestOwnershipRejectsSymlinksAndHardlinks(t *testing.T) {
	for _, target := range []string{"root", "directory", "file", "hardlink"} {
		t.Run(target, func(t *testing.T) {
			base, outside := t.TempDir(), t.TempDir()
			root := filepath.Join(base, "data")
			if err := os.Mkdir(root, 0700); err != nil {
				t.Fatal(err)
			}
			if target == "root" {
				root = filepath.Join(base, "root-link")
				if err := os.Symlink(outside, root); err != nil {
					t.Skip("OS disallows symlink creation")
				}
			} else if target == "directory" {
				if err := os.Symlink(outside, filepath.Join(root, ownershipDirectory)); err != nil {
					t.Skip("OS disallows symlink creation")
				}
			} else {
				if err := os.Mkdir(filepath.Join(root, ownershipDirectory), 0700); err != nil {
					t.Fatal(err)
				}
				external := filepath.Join(outside, "external")
				if err := os.WriteFile(external, []byte("unchanged"), 0600); err != nil {
					t.Fatal(err)
				}
				path := filepath.Join(root, ownershipDirectory, ownershipFilename)
				if target == "hardlink" {
					if err := os.Link(external, path); err != nil {
						t.Skip("OS disallows hardlink creation")
					}
				} else if err := os.Symlink(external, path); err != nil {
					t.Skip("OS disallows symlink creation")
				}
			}
			if file, locked, err := acquireOwnership(root); err == nil || locked || file != nil {
				t.Fatal("linked ownership path accepted")
			}
			if entries, err := os.ReadDir(outside); err != nil || (target == "root" || target == "directory") && len(entries) != 0 {
				t.Fatal("ownership changed symlink target")
			}
		})
	}
}
