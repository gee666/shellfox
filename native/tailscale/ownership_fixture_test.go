package main

import (
	"path/filepath"
	"testing"
)

// A new private-at-birth child, never permission repair of t.TempDir's inherited ACL.
func ownershipTestRoot(t *testing.T) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "private-root")
	ownershipTestDirectory(t, path)
	return path
}
func ownershipTestDirectory(t *testing.T, path string) {
	t.Helper()
	if err := createOwnershipTestDirectory(path); err != nil {
		t.Fatal(err)
	}
}
