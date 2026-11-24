# shellfox-terminal

Reusable PTY transport for Shellfox terminals. It does not parse terminal sequences or open Shellfox production storage.

## Platforms

Interactive sessions run on Windows 10 1809 or newer, Linux and macOS. Other platforms return an error before opening a PTY. Windows and Linux start each shell through a trusted supervisor executable; macOS starts it directly. The Windows Job ownership is below. Linux cgroup or session containment and the macOS process-group cleanup are in [UNIX.md](UNIX.md).

On Windows and Linux the supervisor executable must call `shellfox_terminal::supervisor_entry()` before GUI initialization, hooks, application argument parsing or other work. If it returns `Some(code)`, exit immediately with that code. Set `config.supervisor = Some(std::env::current_exe()?)`. Never substitute an arbitrary executable or a PATH lookup.

## Ownership

The parent creates a noninheritable, unnamed Windows Job with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and no breakaway flags. The portable-pty child is the trusted supervisor, not the user's shell. That supervisor cannot start a command until the parent assigns its original process HANDLE to the Job, verifies membership in that exact Job and releases a random startup event. The supervisor independently checks Job membership. A second nonce-matched event signals READY only after command spawn succeeds. Both startup waits are bounded.

Startup events use 128 random bits from BCrypt and the current token's default DACL. They are not inheritable. This protects the startup protocol from accidental interference, not from a hostile process running as the same user or an administrator. The PTY and Job are not a filesystem, network or privilege sandbox. Programs can deliberately ask external services to do work outside this Job.

`shutdown()` terminates the Job, checks the original supervisor HANDLE is signaled and verifies the Job's active process count is zero. Only then does it finish pipe/console cleanup. Failure retains all remaining owners in the session and disables input. Retry shutdown and refuse normal UI close until it succeeds. A spawn failure may retain a session too. Extract it with `SpawnError::take_session()` and keep the recovery UI alive.

Drop attempts bounded cleanup. If it cannot confirm cleanup, a prestarted worker retains the controller and retries. Drop is not a substitute for checking `shutdown()`. Natural root exit also terminates remaining Job descendants when `try_wait()` is polled. A caller must keep polling, including after it has cached the root exit status.

## Data and environment

`PtyConfig::user(cwd)` is a user terminal. Its environment is the user's own: on Windows a fresh block from `CreateEnvironmentBlock` with the user token, like Windows Terminal (the registry PATH and variables; variables exported in the prompt that started the host don't reach tabs, unlike 0.9), falling back to the host environment if Windows can't build one; on Linux and macOS the host environment. Dropped from the base: `SHELLFOX_*`, `ELECTRON_RUN_AS_NODE`, a Node launcher's `NODE_CHANNEL_*`, and the identity of another terminal (`TMUX`, `TERM_PROGRAM`, `VTE_VERSION`, `KITTY_*`, `WEZTERM_*`, `ALACRITTY_*`, `COLUMNS`, `LINES`, `WINDOWID`). On top, in 0.9's order: `TERM=xterm-256color`, `COLORTERM=truecolor`, `FORCE_HYPERLINK=1`, `PI_HYPERLINKS=1`, `config.env` (Settings and session variables, any well-formed name, PATH included), `config.cli_bin` first on PATH, `SHELLFOX_TERMINAL_MARKER`, then `config.host_paths` (`SHELLFOX_USER_DATA`). On Windows, WSLENV lists those names, host paths with `/p`. Shells run their profiles: PowerShell `-NoLogo -NoExit`, plain `cmd.exe`, Unix shells `-l -i`. The Unix login shell is the account's shell from the user database, then `$SHELL`. Bash gets an OSC 7 cwd hook in front of any inherited `PROMPT_COMMAND`.

`PtyConfig::isolated(root)` is for tests and fixtures. The environment starts empty, home, config, history, cache, temp and pi/npm/pnpm state live below `root`, and shells skip account startup files (`-NoProfile`, `/D /Q`, `--noprofile --norc`). `EnvironmentPolicy::InstalledTools` adds existing executable directories from PATH and the pnpm installation; extra directories go in `tool_directories`. State paths reject symlinks, reparse points and multiply linked files. This is default-state separation, not a sandbox.

`discover_shell_profiles()` returns typed choices and unavailable reasons. On Windows the default is PowerShell 7, then Windows PowerShell, then Command Prompt. An explicit missing choice fails; it never falls back to another shell. Profile IDs are `pwsh`, `windows-powershell`, `cmd`, `wsl` and `login-shell`, parsed strictly. An absolute custom `config.command` is for fixtures and cannot accompany a profile.

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
