package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"maps"
	"os"
	"path/filepath"
	"sync"

	"tailscale.com/atomicfile"
	"tailscale.com/ipn"
)

const stateLimit = 16 * 1024 * 1024

// Tailscale's Windows FileStore resets the primary group of directories named
// "tailscale". That requires rights a normal desktop process need not have.
// Keep its on-disk format, but secure the directory without reassigning groups.
// Enrollment auth keys are not stored here. The durable state contains tsnet's
// private node keys, so its files need the same protection as the directory.
type fileStateStore struct {
	mu    sync.RWMutex
	path  string
	cache map[ipn.StateKey][]byte
}

var _ ipn.StateStore = (*fileStateStore)(nil)

func openStateStore(directory string) (*fileStateStore, error) {
	path := filepath.Join(directory, "tailscaled.state")
	s := &fileStateStore{path: path, cache: map[ipn.StateKey][]byte{}}
	info, err := os.Lstat(path)
	if errors.Is(err, os.ErrNotExist) {
		if err := atomicfile.WriteFile(path, []byte("{}"), 0600); err != nil {
			return nil, errors.New("Cannot create node state")
		}
		return s, nil
	}
	if err != nil || !info.Mode().IsRegular() || info.Size() > stateLimit {
		return nil, errors.New("Invalid node state file")
	}
	if err := protectStatePath(path, false); err != nil {
		return nil, errors.New("Cannot secure node state")
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, errors.New("Cannot read node state")
	}
	// Empty, corrupt or null state must not silently replace an existing identity.
	d := json.NewDecoder(bytes.NewReader(data))
	if d.Decode(&s.cache) != nil || s.cache == nil || d.Decode(new(interface{})) != io.EOF {
		return nil, errors.New("Invalid node state data")
	}
	return s, nil
}

func (s *fileStateStore) ReadState(id ipn.StateKey) ([]byte, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	data, ok := s.cache[id]
	if !ok {
		return nil, ipn.ErrStateNotExist
	}
	return bytes.Clone(data), nil
}

func (s *fileStateStore) WriteState(id ipn.StateKey, data []byte) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if current, ok := s.cache[id]; (data == nil && !ok) || (data != nil && ok && bytes.Equal(current, data)) {
		return nil
	}
	next := maps.Clone(s.cache)
	if data == nil {
		delete(next, id)
	} else {
		next[id] = bytes.Clone(data)
	}
	encoded, err := json.MarshalIndent(next, "", "  ")
	if err != nil || len(encoded) > stateLimit {
		return errors.New("Cannot encode node state")
	}
	if err := atomicfile.WriteFile(s.path, encoded, 0600); err != nil {
		return errors.New("Cannot save node state")
	}
	s.cache = next
	return nil
}
