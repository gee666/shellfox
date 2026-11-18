//go:build !windows

package main

import "os"

// The stable-path firewall change is Windows-only. Existing signed-bundle and
// package-owned non-Windows launch modes keep their existing contract.
func acquireRuntimeExecutableLease() (*os.File, error) { return nil, nil }
