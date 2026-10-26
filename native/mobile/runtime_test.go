package mobile

import (
	"log"
	"os"
	"runtime"
	"strings"
	"testing"

	"tailscale.com/logpolicy"
)

func TestMobileRuntimeHasExplicitPrivateLogsAndTemporaryDirectory(t *testing.T) {
	directory := t.TempDir()
	for _, entry := range os.Environ() {
		name, value, _ := strings.Cut(entry, "=")
		if strings.HasPrefix(name, "TS_") || strings.HasPrefix(name, "TSNET_") {
			t.Setenv(name, value)
		}
	}
	for _, name := range []string{"HOME", "USERPROFILE", "XDG_CACHE_HOME", "TS_LOGS_DIR", "TS_NO_LOGS_NO_SUPPORT", "TMPDIR"} {
		t.Setenv(name, "")
	}
	t.Setenv("TS_AUTHKEY", "tskey" + "-auth-" + "synthetic-secret")
	t.Setenv("TSNET_FORCE_LOGIN", "true")
	writer := log.Writer()
	t.Cleanup(func() { log.SetOutput(writer) })
	if err := configureRuntime(directory); err != nil {
		t.Fatal(err)
	}
	if got := logpolicy.LogsDir(func(string, ...interface{}) {}); got != directory {
		t.Fatal("SDK fell back outside the app-private directory")
	}
	if os.Getenv("TMPDIR") != directory || os.Getenv("TS_NO_LOGS_NO_SUPPORT") != "true" {
		t.Fatal("mobile runtime defaults missing")
	}
	if os.Getenv("TS_AUTHKEY") != "" || os.Getenv("TSNET_FORCE_LOGIN") != "" {
		t.Fatal("inherited enrollment retained")
	}
	if runtime.GOOS != "windows" && os.TempDir() != directory {
		t.Fatal("runtime temporary files would escape app storage")
	}
}
