package main

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"
)

func TestOwnershipProcessEntry(t *testing.T) {
	dir := os.Getenv("SHELLFOX_OWNERSHIP_TEST_DIRECTORY")
	if dir == "" {
		t.Skip("subprocess entry point")
	}
	os.Exit(runOwnership(dir, os.Stdin, os.Stdout))
}

type ownershipChild struct {
	command *exec.Cmd
	stdin   io.WriteCloser
	stdout  *bufio.Reader
	stderr  bytes.Buffer
}

func startOwnershipChild(t *testing.T, directory string, expected bool) *ownershipChild {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	t.Cleanup(cancel)
	child := &ownershipChild{}
	child.command = exec.CommandContext(ctx, os.Args[0], "-test.run=^TestOwnershipProcessEntry$")
	child.command.Env = append(os.Environ(), "SHELLFOX_OWNERSHIP_TEST_DIRECTORY="+directory)
	var err error
	child.stdin, err = child.command.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	stdout, err := child.command.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	child.stdout = bufio.NewReader(stdout)
	child.command.Stderr = &child.stderr
	if err := child.command.Start(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		_ = child.stdin.Close()
		if child.command.ProcessState == nil {
			_ = child.command.Process.Kill()
			_ = child.command.Wait()
		}
	})
	line, err := child.stdout.ReadString('\n')
	if err != nil {
		t.Fatalf("ownership child acknowledgement: %v", err)
	}
	var result struct {
		Locked *bool `json:"locked"`
	}
	if json.Unmarshal([]byte(line), &result) != nil || result.Locked == nil || *result.Locked != expected {
		t.Fatalf("unexpected ownership acknowledgement: %s", line)
	}
	return child
}

func (child *ownershipChild) wait(t *testing.T) {
	t.Helper()
	// Drain before Wait closes the pipe, and require exactly one stdout line.
	remaining, err := io.ReadAll(child.stdout)
	if err != nil {
		t.Fatal(err)
	}
	if err := child.command.Wait(); err != nil {
		t.Fatal(err)
	}
	if len(remaining) != 0 || child.stderr.Len() != 0 {
		t.Fatal("ownership child produced extra output")
	}
}

func TestOwnershipMultiprocessContentionAndEOFRelease(t *testing.T) {
	dir := t.TempDir()
	owner := startOwnershipChild(t, dir, true)
	path := filepath.Join(dir, ownershipDirectory, ownershipFilename)
	before, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	for range 2 {
		contender := startOwnershipChild(t, dir, false)
		contender.wait(t)
	}
	_ = owner.stdin.Close()
	owner.wait(t)
	next := startOwnershipChild(t, dir, true)
	_ = next.stdin.Close()
	next.wait(t)
	after, err := os.Stat(path)
	if err != nil || !os.SameFile(before, after) {
		t.Fatal("ownership restart replaced the lock file")
	}
}

func TestOwnershipProcessDeathReleasesKernelLock(t *testing.T) {
	dir := t.TempDir()
	owner := startOwnershipChild(t, dir, true)
	if err := owner.command.Process.Kill(); err != nil {
		t.Fatal(err)
	}
	if err := owner.command.Wait(); err == nil {
		t.Fatal("killed ownership child exited successfully")
	}
	next := startOwnershipChild(t, dir, true)
	_ = next.stdin.Close()
	next.wait(t)
}
