package main

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"
)

func TestOwnershipDarwinReadsHeldNativeACL(t *testing.T) {
	root, err := filepath.Abs("../../tmp")
	if err != nil {
		t.Fatal(err)
	}
	if err = os.MkdirAll(root, 0700); err != nil {
		t.Fatal(err)
	}
	directory, err := os.MkdirTemp(root, "darwin-helper-acl-")
	if err != nil {
		t.Fatal(err)
	}
	chmod := func(args ...string) error {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		return exec.CommandContext(ctx, "/bin/chmod", append(args, directory)...).Run()
	}
	t.Cleanup(func() { _ = chmod("-N"); _ = os.Remove(directory) })
	file, err := os.Open(directory)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	if err := ownershipACLSafe(int(file.Fd())); err != nil {
		t.Fatalf("plain private directory: %v", err)
	}
	if err := chmod("+a", "everyone deny delete"); err != nil {
		t.Fatal(err)
	}
	if err := ownershipACLSafe(int(file.Fd())); err == nil {
		t.Fatal("private resource with ACL entry accepted")
	}
	if err := chmod("-N"); err != nil {
		t.Fatal(err)
	}
	if err := ownershipACLSafe(int(file.Fd())); err != nil {
		t.Fatalf("ACL removal on the same descriptor: %v", err)
	}
}
