# Upstream review

Reviewed the crates.io `portable-pty` 0.9.0 source archive, not a moving branch.

- Archive URL: https://crates.io/api/v1/crates/portable-pty/0.9.0/download
- Archive SHA-256: `b4a596a2b3d2752d94f51fac2d4a96737b8705dddd311a32b9af47211f08671e`
- License: MIT, copyright 2018 Wez Furlong. The complete notice is retained in `vendor/portable-pty/LICENSE.md`.
- Dependency pin: `portable-pty = '=0.9.0'`.
- Root workspace patch points to `vendor/portable-pty`. That directory preserves upstream layout and its larger files. The under-350-line limit applies to our authored crate files, not preserved third-party sources.

## Local change

In `vendor/portable-pty/src/win/psuedocon.rs`, remove `PSUEDOCONSOLE_INHERIT_CURSOR` from the CreatePseudoConsole flags. Preserve `PSEUDOCONSOLE_RESIZE_QUIRK` and `PSEUDOCONSOLE_WIN32_INPUT_MODE`.

This is a standalone terminal experiment. It does not inherit the outer console's cursor position. With the upstream flag, ConPTY emits `ESC[6n` and prevents the supervisor from reaching READY until a terminal replies. That conflicts with the startup input gate. Disabling cursor inheritance removes that dependency. The transport sends no cursor reply and consumes no parser output during successful startup, so there is no duplicate reply when output reaches Ghostty.

## API findings

`PtySystem::openpty`, `SlavePty::spawn_command`, `MasterPty::try_clone_reader`, `take_writer` and `resize` provide the PTY transport. Unix master reads translate the slave-close EIO into EOF. Master and I/O handles are independently owned, so a reader must keep draining while the Windows pseudoconsole closes.

`src/win/psuedocon.rs` calls CreateProcessW without CREATE_SUSPENDED, Job assignment or an owned Job. `src/win/conpty.rs` owns the pseudoconsole and pipes only. ConPTY is not proof of descendant ownership. Assigning an arbitrary interactive shell after spawn would race its child creation, so Shellfox first starts a trusted waiting supervisor and assigns it before releasing it to spawn a command.

The Windows `WinChild::kill` implementation suppresses errors, and its polling path hides process-query failures and treats exit code 259 as still running. Shellfox instead waits and queries the original process HANDLE and uses its own Job HANDLE. No PID is used to reopen or terminate a process, and no cloned upstream killer is used. Before GO, termination can target the exact original supervisor handle because trusted supervisor code has not released a command. After GO, only the owned Job boundary authorizes tree cleanup.

Upstream Unix child termination does not establish exact descendant ownership. Interactive startup therefore returns an unsupported-platform error before PTY allocation on Unix, including macOS, Linux, Android and iOS. A future Unix implementation needs an equivalent owned supervision design, not cached process-group signaling.
