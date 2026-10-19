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
| `cmd --register <name> <script-path>` | None | Save an absolute script path reference. |
| `cmd <name> [args...]` | `cmd --run <name> [args...]` | Execute the script's current contents. All arguments after the name pass unchanged. |
| `cmd --ls` | `cmd --list` | List custom names, absolute paths and WSL distros. |
| `cmd --remove <name>` | `cmd --rm`, `cmd --unregister` | Remove a reference without deleting the script. |
| `cmd --rename <old-name> <new-name>` | None | Rename a custom command without overwriting another command. |
| `cmd --help` | `cmd -h`, `cmd` | Custom command usage. |

Commands accept `--help`, `-h` or `help` as their subcommand help argument. For example, `shellfox stats -h`, `shellfox tab new help`, `shellfox help tab close` and `shellfox help ssh`. Help works with aliases too. Start aliases still go to the GUI; they are not tab commands.

## Custom script commands

```sh
shellfox cmd --register deploy "/path with spaces/deploy.sh"
shellfox cmd deploy staging "two words"
shellfox cmd --run deploy --help --register literal-argument
shellfox cmd --ls
shellfox cmd --list
shellfox cmd --rename deploy ship
shellfox cmd --remove ship
```

Registration stores an absolute path reference in `<userData>/cli-commands.json`, not a copy of the script. Relative paths resolve against the registering shell's working directory. Each invocation opens the current file, so edits and replacements take effect without registering again. Symlinks remain path references too. Unregistering removes only the reference. Custom commands work without the GUI.

Names start with a letter and contain up to 64 ASCII letters, digits, underscores or hyphens. Names are case-insensitive on every platform. Only duplicate custom names and invalid names are rejected. Custom names may match any top-level command, including `start`, `cmd`, `ssh`, `list` or `help`, because they run only under `shellfox cmd`. Rename cannot overwrite another custom command. Top-level commands never inspect the custom registry and never warn about name collisions. Existing version-1 registrations remain usable without migration.

`shellfox list` and `shellfox ls` still list sessions. Custom management is available only through `shellfox cmd`; the former top-level `register`, `rename`, `unregister`, `commands` and `list --commands` forms are not supported. `cmd --ls` and `cmd --list` list references without contacting the app. WSL entries also show their distro. Paths are JSON-quoted to make spaces and terminal control characters visible.

### Tab completion

`shellfox completion <powershell|bash|zsh>` prints a hook for that shell. It does not edit profiles or install files. New regular embedded PowerShell tabs load the hook automatically when the CLI integration is enabled. Existing tabs, administrator tabs, Bash, zsh and external shells need the explicit loading step below. Unix tabs keep their normal login/rc startup unchanged.

PowerShell 5.1 or newer:

```powershell
shellfox completion powershell | Out-String | Invoke-Expression
```

Bash 3.2 or newer, including Git Bash and WSL:

```bash
source <(shellfox completion bash)
```

zsh, after enabling its completion system:

```zsh
autoload -Uz compinit
compinit
source <(shellfox completion zsh)
```

These commands load generated Shellfox code, not registry contents. Use the trusted installed `shellfox` command. To persist completion, add the appropriate loading line yourself to `$PROFILE`, `~/.bashrc` or `~/.zshrc`. For zsh, keep it after your existing `compinit` call. Reloading a hook is safe. Remove the line to disable it in future shells. PowerShell execution policy still applies to the CLI's `.ps1` shim.

Tab completes `cmd`, custom-management flags and registered names directly on the terminal command line. Names also complete immediately after `--run`, `--remove`, `--rm`, `--unregister` and `--rename`. Completion stops after the selected name. It does not suggest existing names for a new registration or the destination of a rename, and it does not interpret script arguments. Other top-level commands are outside this hook's scope.

Use Tab to fill a matching command name. In PowerShell's default Windows editing mode, repeated Tab presses cycle matches. No additional keybindings are required or installed; your shell's existing Tab behavior is preserved. This is command-line completion, not automatic ghost-text prediction while typing.

The hook queries `shellfox cmd --complete <prefix>` on each name completion. This read-only endpoint returns matching registered names, one per line, without descriptions or headers. An empty prefix requests all names. It needs neither the GUI nor the script files and never runs a registered command. Missing CLI commands, registry errors and empty results produce no dynamic suggestions. The hooks accept only valid command names from the endpoint and never evaluate them as shell source. In WSL, use the Windows-installed shim to complete the Windows registry, or the separate Linux CLI for its own registry.

### Interpreters and arguments

| Script | Execution |
| --- | --- |
| `.sh` on Linux/macOS | `bash -- <absolute-path> [args...]`, with Bash available on PATH. No executable bit required. |
| `.sh` from cmd or PowerShell | Git Bash from the standard Git installation directories. A nonstandard installation can set `SHELLFOX_CLI_GIT_BASH` to its absolute `bash.exe` path. |
| `.sh` from Git Bash | The invoking Git Bash interpreter. POSIX paths are converted with `cygpath` before storage. |
| Script registered through the Windows WSL shim | Store the guest's absolute path and distro. Execute through `wsl.exe --distribution <distro> --cd <caller-directory> --exec`. `.sh` uses `/bin/bash`; other files must be guest executables with a valid shebang. |
| `.ps1` outside WSL | Windows PowerShell 5.1 on Windows, or `pwsh` on Linux/macOS. Pass arguments through a base64 JSON payload and array splatting. |
| Other files on Linux/macOS | Execute the file directly. It needs an executable bit and a valid shebang or native executable format. |
| Other files on Windows | Only `.exe` and `.com` programs. `.bat` and `.cmd` scripts are rejected because cmd reparses arguments as command text. |

`.sh` explicitly means Bash, regardless of its shebang. Use LF line endings. Shellfox executes scripts with your permissions, not in a sandbox. Register only scripts you trust.

Arguments are separate values, never interpolated into a shell command or evaluated with `eval`/`Invoke-Expression`. Empty strings, spaces, quotes, flags, dollar signs and shell metacharacters survive after the calling shell has parsed them. Git Bash receives fully quoted Windows argv, including apostrophes that Node's normal quoting would leave bare. Management flags are interpreted only before the script name. Every argument after the name goes to the script unchanged, including `--help`, `help`, `--register`, `--rm`, `--rename`, `--list` and `--run`. The same rule applies to `cmd --run <name>`. For management help, run `shellfox cmd --help`; `shellfox cmd help` runs a custom script named `help`.

Windows installs both `shellfox.cmd` and `shellfox.ps1`. The PowerShell shim encodes argv before calling Node, avoiding PowerShell 5.1's native-command loss of empty values and embedded quotes. PowerShell normally selects the `.ps1` shim. If execution policy blocks locally generated scripts, allow them under your organization's policy or invoke the shim with `powershell.exe -NoProfile -ExecutionPolicy Bypass -File "<bin>/shellfox.ps1" ...`. `shellfox.cmd` remains available but uses cmd's quoting rules, even when invoked from PowerShell. In cmd, quote values containing `&`, `|`, `<`, `>` or spaces; `%` expansion and caret processing happen in cmd before Shellfox can see the arguments. Prefer the PowerShell or POSIX shim when exact embedded quotes or empty values matter. Windows command-line length limits still apply.

Scripts inherit the invoking working directory, stdin, stdout and stderr. For a WSL reference invoked from Windows, Shellfox converts the Windows caller directory with `wslpath` in the stored distro. From that distro's WSL shim, it uses the guest caller directory unchanged. WSL execution requires Windows interoperability and a version supporting `--cd`. A separately installed Linux CLI has its own Linux registry. It does not share the Windows registry. Missing files, removed distros and missing interpreters produce errors without deleting the registration.

The script's exit code becomes Shellfox's exit code. POSIX signal exits use `130` for SIGINT and `143` for SIGTERM. Registration/registry/execution errors return `1`; invalid management syntax returns `2`. Writes use an exclusive lock and an atomic replacement to avoid losing concurrent registrations. After a crashed writer, check that no registration is running before removing `<userData>/cli-commands.json.lock`.

### Custom-command tests

```sh
pnpm exec vitest run src/main/platform/shellfox-cli*.test.ts
node --test scripts/packaging.test.mjs scripts/shellfox-cli-package.test.mjs
SHELLFOX_TEST_WSL_DISTRO=Debian pnpm exec vitest run src/main/platform/shellfox-cli-scripts.test.ts
```

The CLI packaging smoke builds the production helper through the same build function as release builds, stages the checksum-pinned Windows runtime, and exercises the installed shims under `resources/cli`. It uses the verified cache or downloads the pinned Node assets if needed.

The WSL test is opt-in. It boots the selected distro and writes only fixtures below the project's `tmp/`, using the Windows mount from inside WSL. It does not change startup files or the user's registry. To also run the native Linux helper test, set `SHELLFOX_TEST_LINUX_NODE` to the guest's absolute Node executable. In Git Bash, set `MSYS2_ENV_CONV_EXCL=SHELLFOX_TEST_LINUX_NODE` so MSYS does not convert that Linux path.

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
