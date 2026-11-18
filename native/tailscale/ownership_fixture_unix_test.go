//go:build !windows

package main

import "os"

func createOwnershipTestDirectory(path string) error { return os.Mkdir(path, 0700) }
