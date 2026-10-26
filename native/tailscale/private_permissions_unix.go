//go:build !windows

package main

import "errors"

func validPrivatePlatformPath(path string) bool { return true }

// Node retains its existing uid/mode checks on Unix.
func privatePermissions(path string, directory, protect bool) error {
	return errors.New("private permissions command requires Windows")
}
