package main

import (
	"encoding/json"
	"io"
	"os"
	"path/filepath"
)

const ownershipDirectory = ".host-lock"
const ownershipFilename = "host.lock"

func acquireOwnership(directory string) (*os.File, bool, error) {
	if !filepath.IsAbs(directory) {
		return nil, false, os.ErrInvalid
	}
	file, err := openOwnershipFile(directory)
	if err != nil {
		return nil, false, err
	}
	locked, err := lockOwnershipFile(file)
	if err != nil || !locked {
		_ = file.Close()
		return nil, false, err
	}
	return file, true, nil
}

// The parent owns stdin's write end. EOF releases ownership without PID files,
// polling, port allocation, or stale-lock recovery. The lock file is never removed.
func runOwnership(directory string, input io.Reader, output io.Writer) int {
	file, locked, err := acquireOwnership(directory)
	encoder := json.NewEncoder(output)
	if err != nil {
		_ = encoder.Encode(map[string]string{"error": "Cannot acquire host ownership"})
		return 1
	}
	if !locked {
		if encoder.Encode(map[string]bool{"locked": false}) != nil {
			return 1
		}
		return 0
	}
	defer file.Close()
	if encoder.Encode(map[string]bool{"locked": true}) != nil {
		return 1
	}
	if _, err := io.Copy(io.Discard, input); err != nil {
		return 1
	}
	return 0
}
