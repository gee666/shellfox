# Experimental CLI

`shellfox-cli` is a local client for the Rust experiment host. It uses the service's typed `experiment` API and core session/settings types, not the legacy mobile `ManagerSnapshot` protocol. It does not install a system service or replace the existing Shellfox CLI.

## Start a host

Build with `cargo build --locked -p shellfox-cli`. The binary dispatches the dedicated PTY supervisor, then `shellfox_terminal::raw_process::supervisor_entry()`, before parsing normal arguments or constructing its current-thread Tokio runtime.

Development host commands require explicit absolute `--data-dir` and `--helper` paths plus `--helper-sha256 HEX` before the command. Installed builds use their standard data directory and trusted helper. The hash is exactly 64 hex digits from a trusted helper build or release manifest. It is not a secret. Do not establish trust by hashing an arbitrary executable you find on disk. The service verifies the helper against the expected digest before using it.

Use an existing, initially empty experiment directory, never a production Shellfox directory. `--helper` points to the native helper that checks private directory permissions and host ownership. It is not a shell executable. The host uses this CLI binary itself as its PTY supervisor.

PowerShell example, run from the repository root after building the native helper:

```powershell
$cli = (Resolve-Path target/debug/shellfox-cli.exe).Path
$data = Join-Path (Get-Location) 'tmp/cli-example'
$null = New-Item -ItemType Directory -Force $data
$helper = (Resolve-Path tmp/tailscale/win32-x64/shellfox-tailscale.exe).Path
# Replace this with the SHA-256 supplied by your trusted helper build.
$sha = 'REPLACE_WITH_TRUSTED_HELPER_SHA256'
$common = @('--data-dir', $data, '--helper', $helper, '--helper-sha256', $sha)
& $cli @common host serve
```

`host serve` stays in the foreground. In another terminal, use the same paths for client commands. The host must already be running. Clients do not start it automatically.

```powershell
& $cli @common session create --cwd (Get-Location).Path --profile cmd --title Demo
& $cli @common session list
& $cli @common tab list
& $cli @common attach TAB_ID
& $cli @common host stop
```

Replace `TAB_ID` with the UUID returned by `tab list`. Client commands print JSON, except interactive attach and foreground raw automation. `session list` returns sessions with their tabs; `tab list` returns a flat tab array. Session/tab mutations return the updated snapshot. Both `settings get` and `settings set` return the full global settings object. `host stop` returns `{"stopped":true}` after the service confirms shutdown. Errors go to stderr and return exit code 1.

## SSH

Use `shellfox ssh` to pick a saved connection, or `shellfox ssh --profile "Work server"` to connect by name. A positional name and `-p` also work. The session runs in the current terminal. Use `shellfox-cli ssh` when running the CLI binary directly.

Manage profiles in Settings → SSH connections. The CLI reads the selected profile over authenticated local IPC when the host is running. With no published endpoint, it reads the existing profile store without changing it. It never starts a host or retries using disk after an authentication failure.

## Commands

```text
start [PATH] [--new-window]  # aliases: add, new, create
run [--cwd PATH] [--title TITLE] [--request-id UUID] [--detach] -- EXECUTABLE [ARG...]
runs list
runs show RUN_ID
runs stop RUN_ID
host serve
host stop
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
```

Quote titles containing spaces. Creation requires an explicit profile, so it never silently chooses a platform-inappropriate shell. Built-in launch identifiers are `pwsh`, `windows-powershell`, `cmd`, `wsl`, and `login-shell`. Metadata may retain other opaque IDs, including unavailable profiles. Attach resolves availability on the host and never treats a profile ID as a command. Tab creation can inherit its session's working directory. Creation writes metadata; attach starts the terminal when needed. Closing a session or tab explicitly asks the host to stop any owned terminal before deleting metadata.

`settings set` accepts a partial global `SettingsPatch`, using the same typed settings request as native clients. Missing fields preserve their committed values, including `terminalEnv`. Font and layout preferences belong to the separate device metadata API, not this command. For example, in PowerShell:

```powershell
& $cli @common settings set '{"historyPageSize":16,"terminalProfileId":"cmd"}'
```

Global fields include `accentColor`, `backgroundColor`, `pythonPath`, `adapterId`, `shellId`, `shellExecutable`, `terminalProfileId`, `terminalEnv`, `processRules`, retention hours, and `autoUpdate`. The service resolves changed launch settings through trusted discovery. It rejects arbitrary executables and unavailable selections instead of executing metadata. The parser rejects unknown fields, invalid values, duplicate flags, relative global paths, and extra arguments before contacting the service. `--help` works without paths or a running host.

## Local launch and raw automation

A bare command is a desktop Show intent. `start`, `add`, `new`, and `create` request a new session, resolving a relative directory against the invoking process's cwd. `--new-window` changes the window disposition. These requests go to the one registered desktop coordinator with request-ID deduplication. The CLI waits for the correlated Handled receipt, not merely Queued. It never discovers an acceptance executable or starts another Host. An owner without a registered desktop fails clearly: cross-process headless-to-rendering attachment is not implemented.

`run` sends the executable and argv literally, without converting them to a shell string. It supplies the caller's complete Unicode environment and resolved cwd; unrepresentable arguments/environment fail instead of being replaced or dropped. This differs from quarantined interactive shells: raw automation is an explicit request to run with the caller's environment. Arguments, environment, stdin and stdout/stderr are not persisted or logged.

Foreground `run` preserves separate binary stdout/stderr and returns the target exit code. Stdin is not read until containment and READY are confirmed. Pipe chunks, outstanding OS-write receipts and output credit are bounded; output credit advances only after destination writes complete. Disconnect closes stdin/releases that caller's pipe lease, not the owned process. `--detach` closes stdin and returns the run DTO. Request-ID retries on the same live client must match the original specification; a different specification/owner is rejected. No mutation is automatically replayed after a transport failure.

`runs show` requires a registered native desktop. It publishes a read-only `automation-pipe` tab over the canonical external-byte parser created before raw launch, not a new PTY or a tail-reconstructed parser. It does not invent shell facts or focus every window. Detaching an observer does not kill the run. Explicit stop or metadata close asks the retained raw owner to confirm process-tree cleanup, drains final source data, then retires the parser through its original join. A failed final view is reported separately from confirmed process cleanup, without claiming successful rendering.

Automation remains local-host only. It adds no remote/mobile methods or input authority. The current raw backend is supported only where native ownership proof exists; other platforms fail closed rather than using process-group/PID guesses. Observer-capable remote parity remains design work in `docs/observer-profile-v2-proposal.md`.

## Interactive subset

Attach requires terminal stdin and stdout. It enters a separate screen in raw mode, resizes the host terminal, requests visible-screen ANSI recovery with an exact sequence watermark, then polls live output. Recovery does not promise terminal modes or scrollback. The CLI writes and flushes recovery or live bytes before acknowledging them; Poll can repeat unacknowledged output without dropping it. It writes terminal bytes only to the screen. This crate does not save output, screen snapshots, or input history to files. Each host terminal generation gets its own private home/config root under the explicit data directory. Interactive launches opt into installed executable paths, but do not import the account's Pi settings, login state or credentials.

Ctrl-] detaches locally. Ctrl-C is sent to the remote shell. Client disconnect and attach errors do not send a tab close or host stop request. The host owns terminal lifetime, so detaching leaves the shell running. A competing controller must request `--takeover`; stale attachment errors end the old attach. After client commands, the CLI sends a best-effort disconnect to release the service's client slot without killing terminals.

Input covers UTF-8 characters, common control/Alt keys, cursor/navigation keys, F1 through F12, resize events, and plain-text paste in bounded chunks. Paste forwards text as input, including newlines. It is not a safe-paste preview. Mouse input, application cursor/keypad modes, enhanced keyboard protocols, clipboard integration, reconnect/retry after host failure, and full terminal-client parity are not implemented. A known output sequence gap triggers fresh screen recovery rather than appending incomplete output. The terminal guard restores raw mode and leaves the alternate screen on normal return, errors, and panic unwinding.

The output filter suppresses host-owned device/status/color queries, denied pixel queries, OSC clipboard effects, remote resizes and ConPTY key-mode changes. It preserves bounded fragmented UTF-8/ANSI output and leaves actual keys/paste untouched. This prevents another terminal from answering host-owned queries; it is not a second terminal parser or a full TUI compatibility claim.

The parser and crossterm loop have no Windows-only command branches. Actual launch and endpoint support are determined by the shared service and terminal adapters. The current experiment safely launches owned Windows shells only. Unix login-shell and WSL process ownership remain unsupported; this crate does not add an unsafe fallback. No macOS, Linux, Android, or iOS runtime support is claimed by these Windows checks.

## Checks

```text
cargo test --locked -p shellfox-cli
cargo clippy --locked -p shellfox-cli --all-targets --no-deps -- -D warnings
```

Unit tests cover command parsing, key encoding, and output sequence validation. The Windows owned-host integration test requires a trusted native helper and is ignored by default. To run it in PowerShell:

```powershell
$env:SHELLFOX_CLI_TEST_HELPER = $helper
$env:SHELLFOX_CLI_TEST_HELPER_SHA256 = $sha
cargo test --locked -p shellfox-cli --test local_host -- --ignored --nocapture
```

It starts this crate's actual `host serve` child in a fresh directory under project `tmp`, runs real `settings set/get` CLI processes and validates full global settings JSON, creates a cmd session through the typed client, recovers and acknowledges its screen, sends input, checks repeated Poll output before ACK, takes over from another client, rejects stale input, proves old-client disconnect leaves the terminal usable, closes the tab, and sends typed Stop. It waits for normal host exit and removes its scratch directory. On failure it tries typed Stop before using the owned child's kill handle as a last cleanup step. It never searches for or kills unrelated processes.

The raw pipeline regression additionally needs the owned test-support binary:

```powershell
cargo build --locked -p shellfox-terminal --features test-support --bin shellfox-raw-test-helper
$env:SHELLFOX_RAW_TEST_HELPER = (Resolve-Path target/debug/shellfox-raw-test-helper.exe).Path
cargo test --locked -p shellfox-cli --test automation_host -- --ignored --test-threads=1
```

It uses one real private server and the same typed client for concurrent deferred stdin and consumption-acknowledged output. The owned target fills stdout before reading its bounded stdin, so a sequential listener or blocking owner would deadlock. No output flood is accumulated in the verifier.

These checks do not prove the interactive crossterm loop or cross-platform terminal restoration; those need terminal integration checks on each supported host OS.
