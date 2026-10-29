# Terminal commands

The same CLI controls desktop-owned and headless-owned hosts on Windows, macOS and Linux. In a headless release package, `shellfox update --check` checks for a stable release and `shellfox update` installs and selects a verified matching package for future launches. Keep the original launcher and service paths, then restart the host explicitly when ready. The updater does not stop the host. Both accept `--json`; see [headless updates](headless.md#updates). Session operations belong under `shellfox session`; terminal operations belong under `shellfox terminals`. The namespace shortcuts are `ss` and `tm`. `sessions` is also an alias for `session`, and `cmd` is an alias for `commands`. Root and group help list the aliases alongside the canonical commands.

Run `shellfox` or `shellfox help` for grouped help. There are no top-level session verbs or `tab` namespace. A desktop icon still opens the desktop application separately.

## Output and global options

Human-readable tables and key-value fields are the default, including generic API results. Terminal output uses color on a TTY. Redirected output, `NO_COLOR`, and `TERM=dumb` disable color. Session and terminal IDs are full UUIDs that can be copied directly into commands. UUID prefixes, titles and row numbers are not selectors. Tables use aligned columns when they fit the output terminal. Narrow terminals use labeled records and wrap long values without truncation. The same layout applies to session and terminal lists, machines, generic API data and custom commands. Help and process trees also wrap to the available width. Redirected output has no width limit; JSON output does not change.

```sh
shellfox session list
shellfox session inspect SESSION_UUID
shellfox --host remote-machine session stats SESSION_UUID
shellfox --user-data /absolute/private/data host status
```

| Option | Meaning |
| --- | --- |
| `--host local` | Use the local host, the default. |
| `--host <peer-id-or-hostname>` | Route manager requests through the local host to an approved, connected Tailscale peer. Hostnames must be unambiguous. |
| `--user-data <path>` | Select the host's data directory. An explicit value overrides the installed launcher's default. |
| `--json` | Structured output and errors instead of human formatting. Never prompt. |
| `--yes` | Confirm a destructive operation without an interactive prompt. |

Globals can appear before or after ordinary arguments, including command administration: `shellfox commands --list --json` and `shellfox cmd --remove name --yes --json` work. Required option values and administrative operands are never mistaken for globals. `--` stops option parsing for literal values that begin with a flag.

For script execution, put globals before the script name. Everything after that name is opaque: `shellfox commands deploy --json` passes `--json` to the script, as does the `--run deploy` form. SSH accepts trailing globals, but `shellfox ssh profile --json` reports a structured error because interactive SSH does not support JSON mode.

JSON is opt-in. Each ordinary command writes exactly one compact record to stdout, including errors, with no headings or progress messages on stderr:

```json
{"ok":true,"value":{"stopped":true}}
{"ok":false,"error":{"code":"VALIDATION","message":"A session ID is required.","retryable":false}}
```

Exit status is `0` for success or help, `2` for invalid arguments or missing context, and `1` for failed operations or cancellation. SSH retains the remote exit code, `130` for cancellation, and `255` for connection failure. A failed manager result is never wrapped as a successful value.

`--input` accepts inline JSON, `@file.json`, or `-` to read stdin. JSON files and stdin are limited to 1 MiB. Terminal writes from stdin have a 64 KiB limit. Interactive attach, SSH, and custom script execution reject `--json`; registry management and completion generation support it.

## Updates

`update` always targets the local CLI installation, not the host that handles session commands. `--host local` is accepted; any remote `--host` is rejected before lookup, download or installer dispatch. To update another machine, run its own installed CLI there. `--user-data` does not select a different installation or restart a host.

| Installation | CLI update support |
| --- | --- |
| Headless Windows, macOS, Linux, x64/ARM64 | Verified matching archive, selected for future launches; supports `--json`. |
| Desktop Windows x64 Squirrel | `ShellfoxSetup.exe` installer; requires the installed `Update.exe`. |
| Desktop Linux x64/ARM64 `.deb` | Package installer; the bound executable must belong to the `shellfox` package. May request sudo. |
| Desktop macOS x64/ARM64 `Shellfox.app` | Replaces the bound bundle; may request sudo if its parent is not writable. |
| Desktop Windows ZIP/ARM64, Linux portable ZIP, source checkout | Manual update. No fallback to an unrelated installed copy. |

Desktop `update` and `update --check` use the installed platform launcher and reject `--json`. Use `api getUpdateStatus` and `api downloadUpdate` against a running local desktop host for JSON automation, or use the in-app updater. A directly invoked desktop CLI without a launcher binding reports an unsupported operation. Valid global options work before or after `update`; `shellfox help update` describes both product families without starting an updater. Headless updates leave running hosts untouched; desktop installers have their existing restart/elevation behavior and do not provide that guarantee.

See [headless updates](headless.md#updates) for stable launcher paths, explicit host restart, rollback and account restrictions, and [in-app updates](self-update.md) for the desktop confirmation and verification flow. These are different update mechanisms.

## Session

```sh
shellfox session list
shellfox session create . --title project
shellfox session inspect SESSION_UUID
shellfox session rename SESSION_UUID "new title"
shellfox session pin SESSION_UUID
shellfox session pin SESSION_UUID --off
shellfox session env SESSION_UUID
shellfox session env SESSION_UUID --input '[{"name":"LANG","value":"C"}]'
shellfox session settle SESSION_UUID
shellfox session settle SESSION_UUID --confirm-active --yes
shellfox session unsettle SESSION_UUID
shellfox session delete SESSION_UUID --yes
shellfox session rm SESSION_UUID --yes
shellfox session stats SESSION_UUID
shellfox session processes SESSION_UUID
shellfox session stats --all
shellfox session processes --all --pid 4242
shellfox session focus SESSION_UUID
shellfox session refresh SESSION_UUID
shellfox session clear-error SESSION_UUID
```

`list` lists all sessions by default. `create` uses the caller's working directory unless a path is supplied; on a remote host its default is the remote home directory. Creating sessions does not change desktop selection. New sessions use the host's default terminal profile.

Targeted operations accept a positional session UUID or `--id UUID`. If neither is given, the CLI resolves `SHELLFOX_TERMINAL_MARKER` through the local host's authenticated context resolver. Missing, stale or ambiguous context fails. It never falls back to a selected desktop session, working directory or caller PID.

An explicit ID always wins over the ambient marker and `--current`. Supplying two explicit IDs, even if identical, is an error. Remote targeted commands require an explicit ID. A local marker is never forwarded to another host. Commands run inside that remote host's own terminal naturally use its localhost context instead.

These commands are useful inside a Shellfox terminal:

```sh
shellfox ss inspect
shellfox ss rename "current project"
shellfox ss pin
shellfox ss env
shellfox ss stats
shellfox ss processes
shellfox ss rm --yes
```

`stats` and `processes` require a session ID, current context, or explicit `--all`. They use the same validated `getSessionStats` method locally and remotely. `list` also requests fresh observations. Generic `api getSnapshot` reads the cached manager snapshot instead; it does not force a native tracking poll.

`env --input` replaces the entire saved environment. Without `--input`, it reads it. `settle` and `unsettle` change status without closing terminals or stopping agents. If settling active work requires confirmation, use `--confirm-active --yes`. `delete`, `rm`, and `-rm` are the same operation within the session namespace. Noninteractive deletion requires `--yes`; piping `yes` is not a substitute. Interactive deletion inspects the resolved target before prompting and keeps that UUID pinned for the mutation.

### Filters and metrics

`session list`, `session stats`, and `session processes` accept these filters. Values accept both `--name text` and `--name=text` syntax.

| Filter | Meaning |
| --- | --- |
| `--running` | Session activity is running, not a CPU-utilization measurement. |
| `--idle` | A live session is waiting. Unknown, error and settled sessions do not qualify. |
| `--no-agents` | The reported agent count is zero, not proof that no process exists. |
| `--current` | Select the invoking Shellfox terminal's session unless an explicit ID is supplied. |
| `--id UUID` | Select one session by full UUID. |
| `--name TEXT` | Case-insensitive substring of the session name. |
| `--path TEXT` | Case-insensitive substring of its starting directory, not the shell's live CWD. |
| `--tab UUID` | Include only that terminal and its containing session. |
| `--pid PID` | `processes` only. Match a positive PID within each process domain. |

Filters combine with AND. `--running` and `--idle` conflict. Use `--all` intentionally for aggregate metrics rather than relying on ambient context. `--all` cannot be combined with an explicit session ID or `--current`.

RSS is in MiB. Session RSS sums included terminal measurements; duration is the oldest measured shell root. Closed terminals without stats are excluded from totals but remain visible as saved rows with `n/a`. Closed terminals with pending-cleanup stats still contribute. Missing live-terminal metrics leave the corresponding aggregate unavailable, not zero.

`--pid` filters process rows and selects their containing sessions and terminals. It does not recompute full-terminal RSS or duration. Process output covers observed owned trees, not all OS processes. Local and WSL domains are labeled separately. Short-lived, inaccessible, detached, remote, container or multiplexer activity may be missed. Control bytes in metadata are escaped rather than sent to the terminal.

## Terminals

```sh
shellfox terminals list
shellfox terminals list SESSION_UUID
shellfox terminals list --current
shellfox tm create
shellfox terminals create SESSION_UUID --title shell --profile PROFILE_ID
shellfox terminals rename SESSION_UUID TAB_UUID "new title"
shellfox tm rename TAB_UUID "current-session terminal"
shellfox terminals reorder SESSION_UUID FIRST_TAB_UUID SECOND_TAB_UUID
shellfox tm reorder --current FIRST_TAB_UUID SECOND_TAB_UUID
shellfox terminals write TAB_UUID "text without an Enter key"
shellfox terminals write TAB_UUID --stdin < commands.txt
shellfox terminals write TAB_UUID --take "explicitly switch control before writing"
shellfox terminals close TAB_UUID --yes
shellfox terminals attach TAB_UUID
shellfox tm attach TAB_UUID
```

`tm` is a namespace alias for the same terminals. `create` and `rename` can resolve the containing session from current context when their session ID is omitted. `list` defaults to all terminals; `--current` selects the current session. `reorder` takes the complete saved permutation, including closed terminals. Supply the session ID first, or use `--current` or `--id SESSION_UUID` to treat all positional UUIDs as terminal IDs. Write and close require an explicit terminal UUID.

`attach TAB_UUID` requires an existing terminal UUID and never creates a session or tab. It does not accept `--cwd` or `--session-id`, and it never falls back to desktop selection or ambient session context. The terminal keeps its existing shell and working directory.

Interactive attach requires TTY stdin and stdout and rejects `--json`. Only one client views and controls a terminal at a time. A blocked CLI shows a blank screen and a control message. Ctrl+T explicitly takes the terminal; a fresh replay is displayed before input is enabled. Losing control clears the view and drops stale queued input. Ctrl+] detaches locally; Ctrl+C goes to the shell only while this client owns it. There is no control timeout. An attached waiting client receives control automatically when the owner detaches or disconnects. One-shot `terminals write` refuses an occupied terminal unless `--take` is supplied. EOF, errors, termination signals and connection loss restore raw-input state. Cleanup resets mouse/focus reports, bracketed paste, keypad modes, colors, scroll region and cursor visibility before leaving the local alternate screen.

The CLI replays the saved screen at the host grid before requesting the client's PTY size. It temporarily requests a terminal-window resize with CSI 8 and restores the caller's dimensions, including failed replay. Those temporary changes are not treated as user resizes. If both grids already match, one short resize/restore pair forces a TUI repaint; a 100 ms hold prevents ConPTY from coalescing it. There is no resize heartbeat.

Queries answered by the host parser are not replayed to a second terminal responder. Historical color queries are removed while palette settings remain. Remote window-resize commands and ConPTY's private input mode cannot change the caller's window or keyboard protocol. This prevents nested resize and capability-reply feedback loops. Only the current owner can view terminal contents or change its size and input. Waiting clients receive a fresh replay after taking control or being promoted.

Output is acknowledged after delivery. Input is limited to 64 KiB per frame and 128 KiB pending. Oversized paste or a full queue detaches with an explicit error, not silent truncation. A lost reply may mean input was already delivered. The CLI never reconnects and resends that uncertain input. Inspect the terminal before sending it again. Detaching or exiting a client never closes the shell or stops the host.

### External TTY verification

After the final test build, run this smoke against an isolated real desktop host:

```sh
pnpm build:test
pnpm exec playwright test --project=e2e tests/e2e/cli-attach-tty.spec.ts
```

The smoke creates its sessions and tabs through the window API before launching the built CLI through `node-pty`, not mocked streams. A separate bundled-Node driver loads the staged terminal addon; the Playwright worker never loads it. The harness exchanges JSON lines over ordinary stdio pipes, with only input-write and test-owned kill commands after startup. Evidence records native PTY data/exit events, command acknowledgements, driver errors, stderr and driver exit status. A driver failure never substitutes a terminal exit code. It attaches existing UUIDs through `terminals` and `tm`, checks that a missing UUID fails without mutation, and verifies that no CLI attach changes the session/tab identities. Each client explicitly takes control with Ctrl+T if another viewer owns the terminal, sends a shell command, checks the returned live working directory, and detaches with Ctrl+]. The test checks that the shell remains open after the client exits. A second case starts a resize-driven Node TUI on a pre-sized 90x40 terminal and takes it from an external 90x40 CLI. It checks that the TUI repaints at 90x40 without manually resizing the external TTY. Playwright attaches `external-tty-evidence.json` and `same-grid-repaint.json` with the isolated data directory, CLI/runtime paths, session/tab IDs, shell output, exit codes and repaint counters. It does not access the installed host or register startup tasks. Windows, macOS and Linux require their normal desktop/native build prerequisites. Mobile and headless clients use the same existing manager contracts; the external desktop smoke does not verify those platforms.

## Automation processes

```sh
shellfox run [--cwd PATH] [--title TITLE] -- EXECUTABLE [ARG...]
shellfox run --detach --json -- EXECUTABLE [ARG...]
shellfox runs list --json
shellfox runs show RUN_UUID --json
shellfox runs stop RUN_UUID --yes
```

`run` starts an ordinary piped process in the local desktop or headless host. Foreground mode forwards stdin, separate stdout/stderr bytes, and the exit code without adding output. It rejects `--json`; detached mode closes stdin and returns a run record instead. Arguments after `--` are literal. Invoke a shell explicitly when shell parsing is required.

`runs show` creates or selects a read-only terminal view and returns its session/tab IDs. Viewing does not change the process's pipes. Recent visual replay remains available for finished jobs during the current host lifetime. It is bounded text, not a full transcript. Use foreground pipe capture for exact output.

These commands require an existing local host and reject remote `--host`. Jobs end when their owning host stops, not when an attached desktop client closes. See [headless automation](headless.md#automation-processes) for environment, buffering, retention, and platform details.

## Host

```sh
shellfox host run
shellfox host run --json
shellfox host status --json
shellfox host keep-awake on
shellfox host stop --yes --json
shellfox host service install
shellfox host service install --startup
shellfox host service uninstall --yes
```

An existing desktop-owned or headless-owned host handles commands without opening or focusing a window. With no host, human-readable `session create` in a desktop install retains GUI fallback. JSON mode reports `HOST_UNAVAILABLE` rather than claiming an asynchronous GUI launch created a session. Headless packages never launch Electron or a GUI; start their host explicitly.

`host run` stays in the foreground until stopped or signaled. With `--json`, it writes one readiness record and stays running; a later fatal error is a separate error record. `host status` reports ownership mode, PID, start time and capabilities. `host stop` waits for shutdown and requires confirmation or `--yes`.

`host service` uses the installer shipped with either the desktop CLI or headless package. Linux uses systemd user services, macOS a LaunchAgent, and Windows a same-user limited Scheduled Task, not an SCM service. Without `--startup`, Windows installation is login autostart. The startup option requires elevated registration and uses the current account, never LocalSystem. See [headless deployment](headless.md) for boot-time, account, credential and network limitations. No test or ordinary status command installs OS startup.

## Network, machines and approvals

```sh
shellfox network status
shellfox network enable --hostname workstation
shellfox network login
shellfox network login --auth-key-stdin < private-auth-key
shellfox network login --auth-key-file /protected/auth-key
shellfox machines list
shellfox machines connect PEER_ID
shellfox machines disconnect PEER_ID
shellfox machines forget PEER_ID --yes
shellfox approvals list
shellfox approvals approve PEER_ID
shellfox approvals reject PEER_ID --yes
shellfox approvals revoke PEER_ID --yes
shellfox network disable --yes
```

Login without a key returns a Tailscale login URL when authentication is needed. Auth keys are never accepted in argv, including generic remote-login payloads. Protected files contain raw key text, with an optional trailing newline. On Unix, the file must belong to the current user and have mode `0600` or stricter. On Windows, allow rules may grant access only to its current-user owner, SYSTEM or Administrators. Links and oversized files are rejected.

`machines list` includes only confirmed Shellfox hosts, not every Tailscale device. Saved confirmed hosts can appear offline. Unverified legacy saved targets remain visible in connection status but not this list until discovery succeeds. When an older running local host omits `hostConfirmed`, an already connected peer is included because its completed Shellfox pairing proves the host. Unconnected legacy peers and explicit `hostConfirmed: false` remain excluded. Discovery runs in the host, refreshes the peer map every three seconds and caches negative probes for 30 seconds; it does not pair or request approval.

Approval listing includes saved approvals and pending requests. Reject denies a pending pairing attempt. Revoke removes an existing grant. Being on the same tailnet is not enough to access a Shellfox host. Machine states distinguish waiting for approval, explicit rejection, disconnection and connection errors. Network, machine and approval commands always manage the local host and reject a remote `--host` instead of silently changing the wrong machine.

## Settings and automation

```sh
shellfox settings read
shellfox settings update --input '{"historyPageSize":30}'
shellfox profiles list
shellfox path home
shellfox path resolve "~/project"
shellfox path complete /work/pro
shellfox path create /work/new-project
shellfox api getSnapshot
shellfox api getSessionStats --input '{"sessionId":"SESSION_UUID"}' --json
shellfox api renameSession --input '{"sessionId":"SESSION_UUID","title":"new title"}' --json
shellfox remote api getState --json
```

Settings updates merge supplied fields into current settings and validate the full result. `profiles` lists terminal profiles and availability. Directory helpers run on the selected host; `path create` is an explicit request to create a directory.

`api` accepts every validated `ManagerMethod` from `src/shared/schemas.ts`, including methods without a dedicated spelling. Input defaults to `{}`. Requests and responses use the application schemas. Desktop-only or unavailable OS actions report `UNSUPPORTED`. Remote requests cannot bypass the existing method allowlist. `remote api` exposes validated local remote management and generic peer requests. Destructive methods require confirmation or `--yes` in both generic channels. Human formatting remains the default; add `--json` for structured automation.

## Commands and SSH

```sh
shellfox commands --register deploy "/path with spaces/deploy.sh"
shellfox commands deploy staging "two words"
shellfox commands --run deploy --help --register literal-argument
shellfox commands --list
shellfox commands --rename deploy ship
shellfox commands --remove ship
shellfox ssh --profile production
```

`cmd` remains an alias for `commands`. Either namespace alone shows the same canonical Commands help. Every argument after a script's name is literal, including `--help`, `--json`, `--register` and `--run`. Top-level commands never inspect the registry or collide with custom names. Registry management works without a host.

Registration stores an absolute path reference in `<userData>/cli-commands.json`, not a script copy. Each invocation reads the current file. Names are case-insensitive ASCII identifiers beginning with a letter, up to 64 letters, digits, underscores or hyphens. Rename cannot overwrite an existing name. Unregistering never deletes the script.

`.sh` runs with Bash. Windows uses Git Bash, or the distro saved when registering through its WSL shim. `.ps1` uses Windows PowerShell on Windows or `pwsh` on Unix. Other Unix files need an executable bit and valid executable format. Other Windows files must be `.exe` or `.com`; batch scripts are rejected to avoid reparsing arguments as command text. Scripts inherit the caller's directory and streams, and run with the caller's permissions, not in a sandbox. Their exit status is preserved.

Windows installs `.cmd` and `.ps1` shims. Prefer PowerShell or the POSIX shim when empty arguments or embedded quotes matter; the PowerShell shim uses encoded JSON argv. In cmd, the calling shell's percent expansion, caret handling and metacharacter quoting still apply. WSL registrations preserve the distro and guest path. Cross-distro execution requires Windows interop and WSL support for `--cd`.

### Completion and WSL routing

```powershell
shellfox completion powershell | Out-String | Invoke-Expression
```

```bash
source <(shellfox completion bash)
```

```zsh
autoload -Uz compinit
compinit
source <(shellfox completion zsh)
```

Completion shows canonical groups and their operations, plus custom-management flags and validated script names. It also recognizes aliases already typed. It does not query session IDs or execute registry text. Loading a hook is safe more than once and does not edit profiles. The Debian package installs Bash and zsh hooks system-wide.

The Windows-installed WSL command runs bundled Windows Node against Windows loopback and the installation's Windows data directory. It forwards `SHELLFOX_TERMINAL_MARKER` unchanged through WSLENV and never falls back to Linux Node or Linux user data. A separately installed Linux command controls a Linux host. Use `type -a shellfox` and `hash -r` when diagnosing a shadowed command.

The opt-in WSL regression test is `SHELLFOX_CLI_TEST_WSL=1 pnpm exec vitest run src/main/platform/wsl-control-launcher.test.ts`. It defaults to Debian, selectable with `SHELLFOX_CLI_TEST_WSL_DISTRO`. It boots that distro but writes startup files only in project `tmp/` fixtures. No test changes the user's PATH, startup files, live host installation or preview data.
