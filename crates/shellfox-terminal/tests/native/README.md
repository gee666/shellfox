# Unix ownership acceptance

These tests use disposable PTYs, fixture commands and project-local `tmp/` directories. They do not open installed Shellfox data or target existing application processes.

The public Unix `PtySession` stays disabled. `unix::native_session` compiles only with `test-support`; it is the real PTY integration used by the native fixtures, not proof that Unix interaction is ready to ship.

## Required Cargo registration

The terminal crate registers this fixture binary behind `test-support`. It adds no workspace member:

```toml
[[bin]]
name = "shellfox-unix-test-supervisor"
path = "tests/support/unix_supervisor.rs"
required-features = ["test-support"]
```

## Linux

Use a native Linux x64 or arm64 runner with procfs, same real/effective UID and working `pidfd_open` / `pidfd_send_signal`. Unsupported kernels or blocked syscalls fail preflight before PTY allocation. Do not replace these failures with numeric PID signals.

```sh
cargo test --locked -p shellfox-terminal --features test-support --lib
cargo test --locked -p shellfox-terminal --features test-support --test unix_native -- --ignored --test-threads=1
cargo clippy --locked -p shellfox-terminal --features test-support --all-targets -- -D warnings
```

`PreparedLaunch::linux` checks native pidfds, prepares the waiting socket and cleanup workers, then allocates the PTY. Its trusted waiting executable authenticates the host through `SO_PEERCRED`. The host captures the original supervisor child pidfd and verifies parent, isolated session, birth and marker before sending GO. The helper then becomes a persistent subreaper and launches the shell using Rust's exec-error pipe. A bounded START frame reports the shell identity. Input waits for the final configured executable image, including after an optional systemd-run wrapper. The helper remains alive to contain detached double-forked descendants even if the shell exits before a host monitor tick.

The process monitor must poll while running. Descendant handles survive root exit and session changes. Cleanup freezes held members, expands verified ancestry to a fixed point, confirms the freeze, kills descendants before the root and confirms every pidfd exit. Failed attempts resume every survivor stopped by that attempt and retain all handles for retry. A dead cached root or descendant never authorizes a recycled SID. An unknown remaining session member causes refusal, not a PID fallback. The helper cleans its own descendants without stopping itself, then exits normally only after verified cleanup. Its control connection also detects host loss. Unexpected signalled helper death retains ownership instead of claiming detached descendants are gone.

Optional systemd scopes use the same probed `OOMPolicy=continue` arguments as launch. Scope discovery accepts only absolute PATH entries and trusted executable paths, then falls back to a directly owned command when the user manager refuses the probe. OOM scores only increase, through an open proc-directory descriptor after birth verification. A caller can use `linux::scope::probe` / `scope_arguments` with explicitly configured runner and probe paths. Scopes are memory isolation, not ownership evidence.

## macOS

Run on the actual target architecture. Build the retained active C source, not any retired Shellfox.Native or Shellfox.Linux project:

```sh
node crates/shellfox-terminal/tests/native/build-darwin.mjs
export SHELLFOX_DARWIN_SUPERVISOR="$PWD/tmp/terminal-native/darwin-$(node -p process.arch)/shellfox-terminal-supervisor"
cargo test --locked -p shellfox-terminal --features test-support --lib
cargo test --locked -p shellfox-terminal --features test-support --test unix_native -- --ignored --test-threads=1
cargo clippy --locked -p shellfox-terminal --features test-support --all-targets -- -D warnings
```

`SHELLFOX_NATIVE_OUTPUT` can select another build directory under project `tmp/`. `SHELLFOX_NATIVE_ARCH` must match the native runner. The recipe uses the macOS SDK, checks the Mach-O architecture with lipo, executes strict native capabilities, and writes source/binary SHA-256 evidence. The runtime constructor takes an explicit absolute helper path from verified resources. The package resource path is `resources/terminal-native/shellfox-terminal-supervisor`; the helper-relative path recorded in the recipe manifest is `terminal-native/shellfox-terminal-supervisor`.

The Rust boundary uses a private owned 0700 directory, a random 256-bit token, peer UID verification, bounded deadlines, one strict UTF-8 JSON response through EOF, and pinned supervisor/shell identities. The command receives no supervisor secret. CLOSE must confirm cleanup and endpoint unlinking before disposal. A failed request retains control and the original PTY transport for retry. Only a normal exit reported by the original retained supervisor child handle can resolve a lost CLOSE reply. A signalled supervisor exit is not cleanup proof. There is no cached killpg/PID fallback.

The existing deterministic C model is a separate control-flow check:

```sh
node scripts/darwin-supervisor-model-test.mjs
```

It does not prove macOS signals, PTYs, ABI or libproc behavior.

## Evidence and remaining gate

The native fixture verifies HUP-ignoring descendants through live Unix sockets, explicit close, natural parent-root exit, an unrelated retained fixture process, token removal, private endpoint cleanup and actual exec failure. Linux leaves its leaf in a detached session; macOS creates a separate job-control group inside the supervisor session. Model tests additionally inject stale identities, acquisition failure, partial termination and rollback failure, then retry with the same retained handles.

`.github/workflows/unix-terminal-native.yml` supplies native x64/arm64 Linux and macOS jobs. This workflow has not been run from the Windows implementation host. Cross-compilation and model tests are not native acceptance. Review native CI results and packaged-resource loading before moving the public unsupported-platform gate. The fixture adapter also uses synchronous writes; the production session must reuse the bounded input/output workers before interactive enablement.
