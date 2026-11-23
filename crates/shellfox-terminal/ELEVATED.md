# Retained Windows administrator helper

The `elevated` module ports the active boundary in
`src/main/terminal/windows-elevated.ts`. Its helper is
`native/Shellfox.ElevatedTerminal`, not the retired Shellfox.Native backend.

The Windows profile menu uses the bundled helper's `probe` and `launch` modes,
as Electron does. Only a fresh local add-tab action can start it. Remote clients
cannot list or attach to the tab. This mode loads the account's PowerShell
profile but does not pass session environment overrides to the elevated shell.
Shellfox does not set a consent deadline. Windows controls the prompt, and the
launcher bounds the authenticated handshake through `ready` after approval.
A Windows consent cancellation is reported separately from a post-approval
startup failure. The local desktop and host stderr report the failed stage
without including paths, environment values or terminal input.

## Authority and ownership

The host communicates with the helper launcher over private inherited
stdio. The C# launcher authenticates the elevated peer against the original
process handle returned by ShellExecute. The peer verifies its original parent
and birth. Rust does not accept renderer-supplied pipe names or kill numeric PIDs.

The privileged helper owns a non-breakaway, kill-on-close Job. The host waits
for the helper's ordered `exit` frame, which follows Job cleanup. Launcher
termination, EOF, watchdog death and timeout are not substitutes.

`Session` retains its original `std::process::Child`, private streams and a
resource hold (the opened helper image). It is released only after exit proof
and original launcher/stream cleanup. A retained startup error includes its session; dropping a view transfers
that controller to a cleanup owner started before launch. Last-resort quarantine
never releases unresolved ownership from a timeout or launcher death.

An input-pipe failure disables input but does not discredit valid authenticated
output. The controller keeps reading its eventual final proof. Malformed output
is different: it remains unconfirmed, closes input and drains without trusting
later frames. Untrusted helper error text never enters status or logs.

## Factory and runtime contracts

- `InteractiveHelper::bundled` resolves only
  `resources/elevated-terminal/Shellfox.ElevatedTerminal.exe` beside the
  executable (`../resources` from `bin/`). Debug builds fall back to
  `tmp/terminal-native/<rid>/`.
- `UserApproval::from_user_action(generation)` represents one fresh trusted-host
  approval action. It is consumed by one launch and is not deserializable state.
  Do not manufacture it from a saved setting or an unvalidated renderer request.
- The launch frame is exactly `launch`, `cwd`, `env`, `cols` and `rows`. It
  accepts no executable, script or arbitrary args. The environment is the user's
  own, without Settings or session variables and without CLR hook variables.
- `Session` exposes generation-bound write, resize, poll, begin-close, timeout,
  service and finish operations. Stale operations do not touch transport.
  Ready identity is the logical PowerShell PID/birth from the private channel,
  never the native launcher or guardian PID. These values are metadata, not kill
  authority.
- Drain Data through `ExitConfirmed`, then call `finish`. `phase == Closed` means
  protocol proof has arrived; `cleanup_confirmed()` additionally requires native
  cleanup and release of the session's extra resource hold. A zero-budget finish
  permits nonblocking retry. Errors retain ownership.
- Input is UTF-8, at most 64 KiB per write and 128 KiB pending, with at most 128
  pending IDs. Exact ordered acks release credit, not proof a command executed.
  Escaping or refused backpressure consumes no ID/credit; unknown partial sends
  are never replayed. Close is idempotent, and late Ready cannot enable input.
- Framing, input and output queues are bounded. The decoder holds one partial
  NDJSON frame of at most 262144 bytes. Output stays raw for the single terminal
  parser. Retired cleanup owners discard bytes without answering terminal queries.

The generic `HelperIo` trait is trusted host injection for tests and adapters,
never a renderer or remote IPC capability. Production Windows uses the concrete
stdio owner. `TrustedHash` and the single-image `VerifiedHelper` exist only under
unit-test/test-support configuration for owned Rust fixtures.

## Scoped evidence

On Windows x64:

```text
cargo test -p shellfox-terminal --lib --locked elevated::tests
cargo test -p shellfox-terminal --features test-support --locked --test elevated_stdio
cargo test -p shellfox-terminal --features test-support --locked --test elevated_retention
cargo clippy -p shellfox-terminal --all-targets --features test-support --locked -- -D warnings
```

Library cross-checks passed for macOS x64 with and without test-support, and for
Windows arm64. These are not native UAC, over-the-shoulder approval, elevated Job,
packaged production loader or foreign-platform runtime acceptance. Native testing
still requires separate user permission. All fake artifacts used repository-root
`tmp/` and were removed. No real user shell, distro, credentials or production data
was used by this elevated slice.
