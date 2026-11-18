# Profile-free Windows launch protocol

This additive protocol belongs to the active Windows helper. Legacy `probe`,
`launch`, `serve`, `pipe-run`, and their messages remain unchanged. Electron
continues to use `launch` and its original `powershell.exe -NoLogo` behavior.

## Capability and invocation

Run `probe-isolated` without elevation. It emits exactly this object and a newline:

```json
{"protocol":"shellfox-elevated-isolated","version":1,"shell":"windows-powershell","noProfile":true}
```

This version supports only the OS system directory's
`WindowsPowerShell/v1.0/powershell.exe`, with fixed `-NoLogo -NoProfile -NoExit`
arguments and a constant `-Command` bootstrap. That bootstrap restricts modules to
`$PSHOME/Modules`, checks both already-loaded and discoverable PSReadLine modules,
and sets its history path to
the validated `PSREADLINE_HISTORYFILE`. Any bootstrap error exits the shell rather
than continuing with account history. No payload text enters the script.
It accepts no executable, script, command, or argument fields. `noProfile` means
PowerShell profile scripts are disabled, not that Windows Known Folders or all
programs' account state are redirected. This is not a security sandbox.

Start `launch-isolated` with private redirected stdio. Send one bounded NDJSON
object with exactly these fields:

```json
{"type":"launch-isolated","isolationRoot":"C:\\owned\\experiment","cwd":"C:\\owned\\project","env":{},"cols":80,"rows":24}
```

The empty environment above only illustrates the schema. It is invalid. The
required environment is specified below. Unknown or duplicate fields fail closed,
including case-insensitive environment duplicates. Dimensions remain 2 through
500, inclusive. Existing frame, environment, and input limits still apply.

`isolationRoot`, `cwd`, and environment paths use ordinary absolute local drive
paths. UNC/device paths, alternate data streams, dot components, and ambiguous
trailing dots/spaces are rejected. Rust prepares and owns the root and its state
directories before launching. The helper does not create them. `cwd` may be
outside the root. Root, cwd, state directories, and PATH directories must exist
without reparse points in their ancestors. The root cannot be a drive root.
Existing state files must be regular non-reparse files with one hard link.
Missing state files are accepted below the prepared state directories. These
checks detect existing redirects, not hostile same-user filesystem races.

## Required environment

Every entry below is required, with no extra entries. Names are case-insensitive.
The helper passes a fresh Unicode environment block, never inherited variables.

| Names | Value relative to isolationRoot |
| --- | --- |
| HOME, USERPROFILE, ZDOTDIR | home |
| APPDATA, XDG_CONFIG_HOME | config |
| LOCALAPPDATA, XDG_DATA_HOME | data |
| XDG_CACHE_HOME, npm_config_cache | cache |
| XDG_STATE_HOME | state |
| TMP, TEMP, TMPDIR | temp |
| PI_CODING_AGENT_DIR | pi-agent |
| PI_CODING_AGENT_SESSION_DIR | pi-sessions |
| npm_config_prefix | npm-prefix |
| PNPM_HOME | pnpm-home |
| HISTFILE | state/shell-history |
| PSREADLINE_HISTORYFILE | state/powershell-history |
| npm_config_userconfig | config/npmrc |
| npm_config_globalconfig | config/npm-globalrc |
| GIT_CONFIG_GLOBAL | config/gitconfig |
| GIT_CONFIG_SYSTEM | config/gitconfig-system |

`SystemRoot` and `WINDIR` must equal the parent of the OS system directory.
`COMSPEC` must be that system directory's `cmd.exe`. `PSModulePath` must be only
that directory's `WindowsPowerShell/v1.0/Modules`, without additional entries.
In particular, a Rust builder that includes pwsh modules must narrow its value
for this Windows PowerShell-only capability.

`PATH` must be a nonempty semicolon-separated list of existing absolute local
directories. No empty or relative search entries are accepted. Rust's optional
InstalledTools policy may supply its validated installed executable directories.
`PATHEXT` is exactly `.COM;.EXE;.BAT;.CMD`, `TERM` is `xterm-256color`, and
`COLORTERM` is `truecolor`. Runtime hooks, agent sockets, credentials, and custom
variables are not part of this protocol.

## Authentication and lifetime

The launcher passes `serve-isolated` only through its original ShellExecute runas
invocation. The private pipe still authenticates the original returned process
handle, and the helper authenticates the original parent with its birth proof.
No environment variable selects the mode. The authenticated launch type must
match the invoked mode at both boundaries. Mode mismatches never fall back.

After startup, ready/data/ack/error/exit frames and write/resize/close commands are
unchanged. The shell still starts suspended, joins the non-breakaway kill-on-close
Job, and only then resumes. Cleanup still requires root exit and zero Job members
before an authenticated exit frame. Cancellation, late UAC approval, partial
startup, disconnected cleanup, and unconfirmed ownership rules are unchanged.
There is no new PID-based termination or unauthenticated cleanup acknowledgement.

### Strong isolated exit acknowledgement

Only `launch-isolated` delays the final exit frame. After the authenticated
helper reports Job-zero cleanup and drained output, the launcher closes its
private helper pipe, then waits on the original ShellExecute process HANDLE.
Closing that pipe releases the helper's command reader. The Rust host does not
need to close its stdin while waiting for the reply. The launcher forwards the
same exit fields only after the OS signals that exact peer has exited. Legacy
`launch` keeps its original forwarding timing.

An eight-second wait deadline reports an existing-schema error with code
`failed` and a static peer-exit-unconfirmed message. It is not cleanup proof.
The launcher retains the original owner and stays alive waiting, even after
host stdin EOF/cancellation or broken status output. A late OS exit may produce
a valid final reply. No broker kill policy, PID reopen/poll or proof-by-sleep is
used. Close/abort timers cannot kill the retained final-phase owner.

On isolated startup cancellation, a pending ShellExecute result also stays
owned. A later approved peer cannot authenticate through the closed endpoint;
its original HANDLE must signal exit before a no-launch exit acknowledgement.
Explicit UAC cancellation, error 1223, proves no peer was created. Unknown
startup failures remain unconfirmed. If any launch bytes may have reached the
helper, peer death without the authenticated Job-zero reply never produces an
exit acknowledgement. Rust must retain unresolved ownership in that case.

The stronger meaning belongs to this unreleased isolated mode, not every old
helper that happens to match a probe string. Bind it to the trusted updated
source/full-bundle digest. It does not waive the managed loader-policy gate.

## Build, trust, and checks

Use the project-local supported .NET 10 SDK. See [the runtime and loader
review](SECURITY.md) for official OS/lifecycle evidence and complete-bundle trust.
Both standalone publishes are supported:

```text
node scripts/dotnet-sdk.mjs provision
node scripts/terminal-native.mjs --arch x64
node scripts/terminal-native.mjs --arch arm64
```

Foreign-architecture publishes skip execution. Native builds run only the two
non-elevated probes. The full publish directory is required at runtime. C# code
changes can leave the apphost EXE unchanged while changing the managed DLL.
Authenticate the full published asset set using the trusted packager manifest
and updated source digest, not just the EXE or hashes computed from arbitrary
supplied sibling files. A matching capability response or a fake transport test
does not establish package trust or enable the Rust real-elevation factory.

Focused tests do not invoke UAC or construct a ConPTY:

```text
node scripts/dotnet-sdk.mjs test
node --test scripts/elevated-isolated.test.mjs
```

The .NET tests use worktree-owned `tmp/isolated-helper-tests-*` fixtures and remove
them after each test. They cover strict launch/environment validation, fixed
arguments, aliases, hardlinks, junctions, framing and current-process private pipe
authentication. CLI tests execute only probes, invalid invocations and malformed
launches that fail before ShellExecute. Do not use the existing `self-test` to
validate this change on an account desktop. It starts the legacy profile-loading
shell. No UAC, real elevated shell, or arm64 runtime acceptance is claimed here.
