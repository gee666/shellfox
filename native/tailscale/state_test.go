package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"

	"tailscale.com/ipn"
)

func stateDirectory(t *testing.T) string {
	t.Helper()
	dir := filepath.Join(t.TempDir(), "tailscale")
	if err := prepareState(dir); err != nil {
		t.Fatal(err)
	}
	return dir
}

func TestStateStorePreservesCompatibleIdentityAcrossRestart(t *testing.T) {
	dir := stateDirectory(t)
	path := filepath.Join(dir, "tailscaled.state")
	fixture, _ := json.Marshal(map[ipn.StateKey][]byte{"_machinekey": []byte("original identity")})
	if err := os.WriteFile(path, fixture, 0600); err != nil {
		t.Fatal(err)
	}
	s, err := openStateStore(dir)
	if err != nil {
		t.Fatal(err)
	}
	got, err := s.ReadState(ipn.MachineKeyStateKey)
	if err != nil || string(got) != "original identity" {
		t.Fatal("existing identity lost")
	}
	if err := s.WriteState("profile", []byte("new profile")); err != nil {
		t.Fatal(err)
	}
	reopened, err := openStateStore(dir)
	if err != nil {
		t.Fatal(err)
	}
	for key, expected := range map[ipn.StateKey]string{"_machinekey": "original identity", "profile": "new profile"} {
		got, err := reopened.ReadState(key)
		if err != nil || string(got) != expected {
			t.Errorf("state %s lost", key)
		}
	}
}

func TestStateStoreClonesBuffersAndDeletesKeys(t *testing.T) {
	dir := stateDirectory(t)
	s, err := openStateStore(dir)
	if err != nil {
		t.Fatal(err)
	}
	input := []byte("durable")
	if err := s.WriteState("key", input); err != nil {
		t.Fatal(err)
	}
	input[0] = 'X'
	first, _ := s.ReadState("key")
	first[0] = 'Y'
	got, _ := s.ReadState("key")
	if string(got) != "durable" {
		t.Fatal("state aliases caller buffers")
	}
	if err := s.WriteState("key", nil); err != nil {
		t.Fatal(err)
	}
	if _, err := s.ReadState("key"); !errors.Is(err, ipn.ErrStateNotExist) {
		t.Fatal("deleted key remains")
	}
	reopened, err := openStateStore(dir)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := reopened.ReadState("key"); !errors.Is(err, ipn.ErrStateNotExist) {
		t.Fatal("deleted key persisted")
	}
	if err := s.WriteState("empty", []byte{}); err != nil {
		t.Fatal(err)
	}
	if got, err := s.ReadState("empty"); err != nil || got == nil {
		t.Fatal("empty value confused with deleted key")
	}
}

func TestStateStoreWriteFailureDoesNotCommitMemoryOrDisk(t *testing.T) {
	dir := stateDirectory(t)
	s, err := openStateStore(dir)
	if err != nil {
		t.Fatal(err)
	}
	if err := s.WriteState("key", []byte("before")); err != nil {
		t.Fatal(err)
	}
	originalPath := s.path
	before, _ := os.ReadFile(originalPath)
	// Replacing a directory is rejected on every OS, even for an elevated CI user.
	s.path = dir
	if err := s.WriteState("key", []byte("after")); err == nil {
		t.Fatal("failed disk write reported success")
	}
	if err := s.WriteState("key", nil); err == nil {
		t.Fatal("failed deletion reported success")
	}
	if got, _ := s.ReadState("key"); string(got) != "before" {
		t.Fatal("failed disk write committed to memory")
	}
	after, _ := os.ReadFile(originalPath)
	if !bytes.Equal(before, after) {
		t.Fatal("failed write changed durable state")
	}
	entries, _ := os.ReadDir(dir)
	for _, entry := range entries {
		if strings.Contains(entry.Name(), ".tmp") {
			t.Error("failed write leaked temporary state")
		}
	}
}

func TestStateStoreRejectsCorruptionWithoutReplacingIdentity(t *testing.T) {
	for _, bad := range []string{"", "not-json", "null", "[]", `{"key":"not base64!"}`, `{} {}`, strings.Repeat("x", stateLimit+1)} {
		dir := stateDirectory(t)
		path := filepath.Join(dir, "tailscaled.state")
		if err := os.WriteFile(path, []byte(bad), 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := openStateStore(dir); err == nil {
			t.Error("corrupt state accepted")
		}
		after, err := os.ReadFile(path)
		if err != nil || string(after) != bad {
			t.Fatal("corrupt state overwritten")
		}
	}
	dir := stateDirectory(t)
	if err := os.Mkdir(filepath.Join(dir, "tailscaled.state"), 0700); err != nil {
		t.Fatal(err)
	}
	if _, err := openStateStore(dir); err == nil {
		t.Fatal("directory accepted as node state")
	}
}

func TestStateStoreSerializesConcurrentWrites(t *testing.T) {
	dir := stateDirectory(t)
	s, err := openStateStore(dir)
	if err != nil {
		t.Fatal(err)
	}
	var workers sync.WaitGroup
	for i := range 12 {
		workers.Add(1)
		go func() {
			defer workers.Done()
			key := ipn.StateKey(fmt.Sprintf("key-%d", i))
			if err := s.WriteState(key, []byte(key)); err != nil {
				t.Error(err)
			}
		}()
	}
	workers.Wait()
	reopened, err := openStateStore(dir)
	if err != nil {
		t.Fatal(err)
	}
	for i := range 12 {
		key := ipn.StateKey(fmt.Sprintf("key-%d", i))
		if data, err := reopened.ReadState(key); err != nil || string(data) != string(key) {
			t.Errorf("concurrent write %s lost", key)
		}
	}
}
