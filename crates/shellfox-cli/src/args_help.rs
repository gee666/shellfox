pub const HELP: &str = "Experimental Shellfox CLI

Usage: shellfox-cli --data-dir ABSOLUTE_DIR --helper ABSOLUTE_EXE --helper-sha256 HEX COMMAND

  start [PATH] [--new-window]    (aliases: add, new, create)
  run [--cwd PATH] [--title TITLE] [--request-id UUID] [--detach] -- EXECUTABLE [ARG...]
  runs list
  runs show RUN_ID
  runs stop RUN_ID
  host serve
  host stop
  host power status|on|off       (existing running local Host only)
  host network status|on|off [--hostname LABEL]
  host network login [--auth-key-stdin]
  host network cancel|continue ATTEMPT_UUID
  host network peer PEER connect|disconnect|reconnect|approve|reject|revoke|forget
  session list
  session create --cwd ABSOLUTE_DIR --profile PROFILE [--title TITLE]
  session rename SESSION_ID TITLE
  session close SESSION_ID
  tab list [SESSION_ID]
  tab create SESSION_ID --profile PROFILE [--cwd ABSOLUTE_DIR] [--title TITLE]
  tab rename TAB_ID TITLE
  tab close TAB_ID
  settings get
  settings set JSON_PATCH
  attach TAB_ID [--takeover]
  peer PEER sessions|tabs [SESSION]|create --cwd HOST_ABSOLUTE_PATH [--title TITLE]|attach TAB [--takeover]

Global flags must precede COMMAND. No default data directory or helper.
--remote-state-dir ABS overrides the node leaf under DATA_DIR/rust-service-v1/go-network.
--remote-hostname LABEL overrides its initial OS-hostname-derived DNS label.
--remote-control-url HTTP_LOOPBACK_ORIGIN selects a trusted local test control server; default none.
--package-manifest ABS --package-manifest-sha256 HEX optionally verifies a stable portable inventory.
Its expected digest must come from the trusted producer receipt, never a runtime self-hash.
Inventory alone does not enable installation: a validated Host launch context is also required.
Without configured resources, System remains query-only; private outputs are not install resources.
--ssh-program ABS --ssh-program-sha256 HEX, --ssh-keyscan ABS --ssh-keyscan-sha256 HEX,
--ssh-askpass-cli ABS --ssh-askpass-cli-sha256 HEX configure SSH only as a complete pinned set.
Digests must come from trusted producers; askpass must be a CLI with the existing SSH dispatcher.
No SSH executable, askpass or personal profile discovery is performed at startup.
--source-factory local|native0 selects NEW source creation; local preserves sudo.
Native0 requires an experimental-shellfox-native build and uses a finite source preset.
--helper-sha256 must be the trusted helper build's SHA-256, exactly 64 hex digits.
Built-in launch profiles: pwsh, windows-powershell, cmd, wsl, login-shell.
Other profile IDs are metadata; attach requires a safely available discovered profile.
Attach requires a terminal; Ctrl-] detaches without closing the shell.
Network operators use an existing authenticated local Host and print its actual state, not completion.
Login keys require --auth-key-stdin with redirected input; never place secrets in arguments.
See crates/shellfox-cli/README.md for the supported subset.";
