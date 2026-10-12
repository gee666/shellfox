# Terminal commands

Enable the shellfox command in Settings on Windows and loose Linux/macOS builds. Debian packages include `/usr/bin/shellfox`. The launcher uses the app's exact user-data directory.

`start` launches or forwards to the GUI. Session controls require the app to be running; they do not start a second manager or open its window. Help and version work without the app. SSH stays independent of the app and uses saved connections. `update` uses the existing platform updater.

## WSL routing

The Windows-installed WSL command invokes bundled Windows `node.exe` with the manager's Windows user-data path. It reads the Windows control endpoint and connects on Windows loopback, not the guest's loopback or Linux home directory. The shim forwards `SHELLFOX_TERMINAL_MARKER` unchanged through WSLENV, replacing conflicting path/direction flags and duplicate entries while preserving unrelated entries. Missing interop or a missing Windows runtime fails without a Linux helper fallback.

Use the Windows-installed command to control the Windows app. A separately installed Linux command controls a Linux manager, not the Windows app. If an older Linux launcher shadows the Windows shim, reload the startup file Shellfox updated and run `hash -r`, or restart the shell. Check the command with `type -a shellfox`.

The isolated Windows-loopback regression test can be run with `SHELLFOX_CLI_TEST_WSL=1 pnpm exec vitest run src/main/platform/wsl-control-launcher.test.ts`. It defaults to Debian; set `SHELLFOX_CLI_TEST_WSL_DISTRO` to choose another distro. It boots that distro but writes startup files only in a project `tmp/` fixture home and never changes the user's PATH or startup files.

## Commands and aliases

| Command | Aliases | Behavior |
| --- | --- | --- |
| `start [path]` | `add`, `new`, `create` | Create a GUI session. Path defaults to the caller's working directory. |
| `ls [filters]` | `list`, `sessions` | Table of live session name, full UUID, starting path and saved tab count. Closed tabs count too. |
| `stats [filters]` | `status`, `usage` | Session and tab RSS and duration. |
| `processes [filters]` | `proc`, `ps` | Session, tab and process hierarchy with PID, parent PID, domain, RSS and duration. |
| `delete [UUID]` | `remove`, `rm` | Inspect, confirm, close owned terminals and delete the session. |
| `archive [UUID]` | `settle` | Inspect, confirm, close owned terminals and move the session to history. |
| `tab open` | `tab add`, `tab new` | Open a tab in the invoking terminal's session with that session's defaults. |
| `tab close` | `tab remove`, `tab rm` | Close the invoking owned tab. |
| `keep-alive` | `keepalive`, `awake` | Toggle the existing app sleep-prevention switch. The switch is not persisted across app restarts. |
| `ssh [--profile name]` | `ssh name`, `ssh -p name` | Connect to a saved SSH connection; without a name, show the picker. |
| `update [--check]` | None | Install the latest release, or only check with `--check`. |
| `--version` | `-v`, `version` | Print the installed version. |
| `help [command]` | `--help`, `-h` | Main help or help for a command. |

Commands accept `--help`, `-h` or `help` as their subcommand help argument. For example, `shellfox stats -h`, `shellfox tab new help`, `shellfox help tab close` and `shellfox help ssh`. Help works with aliases too. Start aliases still go to the GUI; they are not tab commands.

## Filters

`ls`, `stats` and `processes` accept these filters. Options with values accept either `--name text` or `--name=text` syntax. Quote paths and names containing spaces.

| Filter | Meaning |
| --- | --- |
| `--running` | Session activity is `running`. This is tracked activity, not CPU utilization. |
| `--idle` | A live session's activity is `waiting`. Unknown, error and archived sessions do not qualify. This does not prove a shell is waiting for user input. |
| `--no-agents` | The reported session agent count is zero. Unknown tracking can also report zero; this is not proof that no agent or process exists. |
| `--current` | Restrict to the session containing the invoking Shellfox terminal marker. |
| `--id UUID` | Exact session UUID. |
| `--name TEXT` | Case-insensitive substring of the session name, not a tab or process name. |
| `--path TEXT` | Case-insensitive substring of the session's starting path. It is not the shell's live working directory. |
| `--tab UUID` | Exact tab UUID. Show only that tab and its containing session; the `ls` tab count then counts only that matching tab. |
| `--pid PID` | `processes` only. Show matching positive integer PIDs and their containing tabs/sessions. Filter process rows without recomputing full-tab RSS/duration. PIDs can repeat across local and guest domains; output labels the domain. Nonmatching ancestors are not displayed. |

Filters combine with AND. `--running` and `--idle` are mutually exclusive. State and agent filters apply to the whole session, even when `--tab` selects one tab. `--current --id UUID` is an intersection, not an override. Repeated options, missing values and unsupported filters return a usage error.

IDs are full UUIDs, not table row numbers, titles or UUID prefixes. Get session UUIDs from `ls` and tab UUIDs from `stats` or `processes`.

```sh
shellfox ls --running --no-agents --path project
shellfox sessions --name "web app"
shellfox stats --current
shellfox usage --id c36951c8-03f5-49c7-bb11-ad8828044a31
shellfox proc --tab c36951c8-03f5-49c7-bb11-ad8828044a33 --pid 4242
```

## Metrics and output

RSS is in MiB. A session's RSS sums its included tab measurements. Its duration is the maximum included tab duration, the age of the oldest measured shell root. Process duration is that process's elapsed lifetime.

Closed tabs without stats are excluded from session totals, but remain visible as saved tab rows with `n/a` measurements. Closed tabs that still have stats are included, for example while owned descendant cleanup is pending. Missing stats for a live, launching or launch-uncertain tab still keep session totals unavailable. A missing RSS or duration affects only that corresponding session metric. No included measurements means `n/a`, not zero. Tracking failure reasons appear below tabs.

`--pid` filters process rows and selects their containing tabs/sessions; it does not recompute tab metrics from those matching rows. Session totals aggregate the full measurements of the included tabs. They can therefore include processes hidden by the PID filter. `--tab` restricts which tab contributes to the session total.

Process output covers observed app-owned trees, not all OS processes. Domain labels distinguish local and WSL processes. Short-lived, inaccessible, detached, remote, container or multiplexer activity may be missed. RSS is not a deduplicated physical-memory measurement. Control bytes in names, paths, domains and errors are printed as visible Unicode escapes so they cannot issue terminal commands.

## Current terminal and destructive commands

Current context comes only from `SHELLFOX_TERMINAL_MARKER`. The app resolves it to a live owned terminal. There is no fallback to a selected GUI session, caller directory or caller PID. Missing, stale or ambiguous context fails.

`delete` and `archive` accept a positional session UUID or `--id UUID`. Without an ID, they use current context; `--current` makes that explicit. An ID and `--current` cannot be combined for these mutations.

Both commands inspect the target and print session/tab metrics and observed processes first. Then they require TTY input and output and an explicit `y` or `yes`, case-insensitively. Blank input, any other answer, EOF and non-TTY input/output cancel without a destructive request. There is no `--yes` or force flag, and piping `yes` does not bypass confirmation. The confirmed request uses the inspected session UUID, not a second lookup of the current marker. Failed inspection never reaches confirmation. Terminal closure can still fail if ownership or platform prerequisites are unavailable; the command reports the service error.

```sh
shellfox archive c36951c8-03f5-49c7-bb11-ad8828044a31
shellfox rm --current
shellfox tab add
shellfox tab close
```

Tab commands are context-only. They accept optional `--current`, but not UUID targets or read filters. `tab close` closes the invoking tab without the session-level delete/archive prompt.

Exit codes are `0` for success/help/version, `2` for invalid arguments or missing current context, and `1` for cancellation or control-service failure. SSH retains its remote exit codes, `130` for cancellation and `255` for connection failures.
