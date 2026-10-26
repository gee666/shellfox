package mobile

import (
	"errors"
	"io"
	"log"
	"os"
	"strings"

	"tailscale.com/logtail"
)

func configureRuntime(directory string) error {
	// Enrollment comes only from the bridge, never ambient desktop variables.
	for _, entry := range os.Environ() {
		name, _, _ := strings.Cut(entry, "=")
		if strings.HasPrefix(name, "TS_") || strings.HasPrefix(name, "TSNET_") {
			_ = os.Unsetenv(name)
		}
	}
	// NewLocalBackend unconditionally asks LogsDir for local socket-stat state,
	// even when uploads are disabled. Mobile app processes have no HOME/cache
	// defaults. Use an explicit private directory instead of falling back to /tmp.
	if os.Setenv("TS_LOGS_DIR", directory) != nil || os.Setenv("TMPDIR", directory) != nil {
		return errors.New("Cannot configure embedded runtime")
	}
	_ = os.Setenv("TS_NO_LOGS_NO_SUPPORT", "true")
	logtail.Disable()
	log.SetOutput(io.Discard)
	return nil
}
