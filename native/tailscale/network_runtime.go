package main

import (
	"fmt"
	"io"
)

const networkRuntimeFailure = "Shellfox network helper could not start. Stop other Shellfox remote hosts and retry; check local application-data permissions if the problem persists."

// This dispatch stays offline. Only the child at the stable Windows path starts tsnet.
func runNetworkRuntime(args []string, stderr io.Writer) int {
	channel := ""
	if len(args) == 1 && args[0] == "testing" {
		channel = "testing"
	} else if len(args) != 0 {
		_, _ = fmt.Fprintln(stderr, networkRuntimeFailure)
		return 1
	}
	if code, err := launchNetworkRuntime(channel); err == nil {
		return code
	}
	_, _ = fmt.Fprintln(stderr, networkRuntimeFailure)
	return 1
}
