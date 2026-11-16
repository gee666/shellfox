//go:build !windows

package main

import "os"

func launchNetworkRuntime(channel string) (int, error) {
	return 1, os.ErrInvalid
}
