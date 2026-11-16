package main

import (
	"bytes"
	"testing"
)

func TestNetworkRuntimeRejectsArgumentsWithoutLeakingThem(t *testing.T) {
	for _, args := range [][]string{{"secret-path", "secret-key"}, {"production"}, {"../testing"}, {"testing", "extra"}} {
		var stderr bytes.Buffer
		if code := runNetworkRuntime(args, &stderr); code != 1 {
			t.Fatalf("exit code = %d", code)
		}
		if stderr.String() != networkRuntimeFailure+"\n" {
			t.Fatal("network launcher did not return its fixed actionable error")
		}
	}
}
