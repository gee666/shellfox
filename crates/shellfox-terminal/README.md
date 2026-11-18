# shellfox-terminal

Reusable PTY transport for the isolated terminal experiment. It does not parse terminal sequences or open Shellfox production storage.

## Platform gate

Interactive sessions currently require Windows 10 1809 or newer and a trusted supervisor executable. Other platforms return an error before opening a PTY. No direct-child or cached PID/process-group fallback exists. This is not cross-platform lifecycle parity.

The supervisor executable must call `shellfox_terminal::supervisor_entry()` before GUI initialization, hooks, application argument parsing or other work. If it returns `Some(code)`, exit immediately with that code. Set `config.supervisor = Some(std::env::current_exe()?)`. Never substitute an arbitrary executable or a PATH lookup.

## Ownership

The parent creates a noninheritable, unnamed Windows Job with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and no breakaway flags. The portable-pty child is the trusted supervisor, not the user's shell. That supervisor cannot start a command until the parent assigns its original process HANDLE to the Job, verifies membership in that exact Job and releases a random startup event. The supervisor independently checks Job membership. A second nonce-matched event signals READY only after command spawn succeeds. Both startup waits are bounded.

Startup events use 128 random bits from BCrypt and the current token's default DACL. They are not inheritable. This protects the startup protocol from accidental interference, not from a hostile process running as the same user or an administrator. The PTY and Job are not a filesystem, network or privilege sandbox. Programs can deliberately ask external services to do work outside this Job.

`shutdown()` terminates the Job, checks the original supervisor HANDLE is signaled and verifies the Job's active process count is zero. Only then does it finish pipe/console cleanup. Failure retains all remaining owners in the session and disables input. Retry shutdown and refuse normal UI close until it succeeds. A spawn failure may retain a session too. Extract it with `SpawnError::take_session()` and keep the recovery UI alive.

Drop attempts bounded cleanup. If it cannot confirm cleanup, a prestarted worker retains the controller and retries. Drop is not a substitute for checking `shutdown()`. Natural root exit also terminates remaining Job descendants when `try_wait()` is polled. A caller must keep polling, including after it has cached the root exit status.

## Data and environment

Construct `PtyConfig::new(absolute_existing_isolation_root)`. Initially cwd equals that root. Set `config.cwd` to a project directory without changing `config.isolation_root`. Home, config, history, cache and temporary files stay below the isolation root, not in the project. The crate creates state subdirectories but never deletes caller-owned directories.

`discover_shell_profiles()` returns typed choices and unavailable reasons. On Windows the default follows the active app's order, PowerShell 7 then Windows PowerShell. Command Prompt is the last available local default. PowerShell uses `-NoLogo -NoProfile -NoExit`, built-in module paths and an explicit isolated PSReadLine history path. Command Prompt uses `/D /Q` to disable AutoRun. Set `config.profile = Some(ShellProfileId::Cmd)` for cmd syntax. An explicit missing or unavailable choice fails, it never falls back to a different shell. WSL is unavailable because a Windows Job does not own guest descendants. Other platforms remain gated pending their own process-tree supervision.

Profile IDs are `pwsh`, `windows-powershell`, `cmd`, `wsl` and `login-shell`. Their strict `FromStr` implementation rejects command text and unknown IDs. Profile discovery checks executable files and directories, never runs a tool or opens production Shellfox settings. An absolute custom `config.command` is available for controlled fixtures and cannot accompany an explicit profile.

The child environment starts empty. `EnvironmentPolicy::Minimal`, the default, supplies system command lookup and isolated state. Interactive clients can explicitly select `EnvironmentPolicy::InstalledTools`. It adds only existing absolute executable directories discovered from PATH and the pnpm installation, with case-insensitive Windows deduplication. Empty, relative, missing and NUL-containing entries do not enter PATH. The active app's standard Shellfox CLI binding directory is excluded. Windows canonical paths use ordinary DOS/UNC spelling for lookup, cwd and state variables because verbatim paths break PowerShell script authorization and Windows PowerShell path providers. No tool executes during discovery. Extra caller-selected lookup directories go in `config.tool_directories`, which requires InstalledTools and validates every directory.

Installed binaries and writable state are separate. The real pnpm installation directory enters PATH so its relative pi/node launchers resolve, but child `PNPM_HOME`, npm prefix/config/cache, `PI_CODING_AGENT_DIR` and `PI_CODING_AGENT_SESSION_DIR` point below the isolation root. Home, application data, XDG paths, history and temp do too. No account authentication, application settings, provider environment, host bindings, agent socket, `NODE_OPTIONS`, inherited `PSModulePath` or startup hook enters this environment. `config.env` permits explicit custom additions, but rejects case-insensitive aliases of policy-controlled keys and hook/state variables. It cannot restore production homes or host markers accidentally. Policy-controlled state directories and known npm/git/history files reject preexisting symlinks, reparse points, multiply linked files, unexpected file types and canonical paths outside the isolation root before child creation. This check is not a defense against a hostile process racing under the same account.

Pi can be discovered without importing account credentials. It will need separately authorized credentials/settings before account-backed use. A machine's script execution policy may require `pi.cmd` instead of its `pi.ps1` shim; the experiment does not bypass that policy. This is default-state separation, not a filesystem or network sandbox. Executed programs retain the account's file permissions and can access installed tool directories and other account files explicitly.

`try_read()` returns `Ok(None)` when no event is available, ordered `Data`, then `Eof` or `ReadError`, and finally `TryRecvError::Disconnected`. Data chunks are at most 4096 bytes. The queue holds at most 64 events, plus one pending chunk and the reader's 4096-byte buffer. Backpressure reaches the OS pipe. Drain a bounded amount per UI frame, feed the sole terminal parser, then write its replies.

`write()` accepts at most 64 KiB through a one-request worker and waits at most 100 ms for completion. Timeout disables further public input and reports that delivery may be partial. Do not blindly replay a timed-out write. Shutdown cancels synchronous I/O using the owned worker thread HANDLE. `resize()` validates dimensions in 1..=32767. Neither API accepts input before READY or during cleanup.

Poll `try_wait()` every frame to detect root exit and contain descendants, but keep draining output until `Eof`. Root exit alone does not mean the output has drained. For a user-requested close, call `begin_shutdown()` to disable input and terminate owned processes while retaining the reader and queued output. Keep polling and reading through EOF, then call `shutdown()` to join workers. A failed begin-close retains ownership and input remains disabled until cleanup succeeds. Console close runs separately while the reader drains, since ConPTY close can block. Immediate `shutdown()` discards unread output to release a blocked producer; reserve that route for errors or explicit output cancellation.

## Checks

```text
cargo check -p shellfox-terminal --locked
cargo test -p shellfox-terminal --features test-support --locked
cargo clippy -p shellfox-terminal --all-targets --features test-support --locked -- -D warnings
```

The optional `test-support` feature builds a fixture executable. Tests use real short-lived PTYs and per-test directories under the repository's `tmp/`. They cover final output, bounded reads, cancellation of a full queue, draining close, write/resize, repeated close, partial startup failure, missing-GO timeout, injected assignment denial, retained ownership after injected cleanup failure, natural root exit with a remaining descendant and noninterference with an unrelated owned test process. Shell tests run actual available PowerShell profiles, check `ls`, resolve an owned PATH executable and installed pi/node launchers, run a scratch-owned PowerShell script and harmless node arithmetic, and verify that shell state does not enter a separate project cwd. With the user's authorization, installed pi's standard `--version` flag also ran under both PowerShell profiles with offline startup and fresh private state, returning `1.5.2`. No interactive agent, account login, authentication import or network operation was requested. Metadata assertions use a typed JSON file in owned scratch rather than raw wrapped ConPTY output. See `UPSTREAM.md` for the portable-pty API and license review.
