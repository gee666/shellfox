# Headless Shellfox

The headless package is the Shellfox host and CLI without a desktop window. It runs on Windows, macOS and Linux, x64 and ARM64 where a release archive exists, and needs no display server, Node runtime or Tailscale installation. It contains `bin/shellfox-cli`, the embedded Tailscale helper and the platform terminal helpers.

A desktop window, a remote phone or `shellfox terminals attach` all show the host's real PTY. None of them copy the shell.

## Start a server

Extract `shellfox-headless-<version>-<platform>-<arch>` and run its launcher:

```sh
./shellfox host run
```

Windows has `shellfox.cmd` and `shellfox.ps1`. Don't run the host as root or Administrator just to host ordinary terminals.

`host run` stays in the foreground until `host stop`, Ctrl+C, SIGTERM or SIGHUP. Use a startup manager for unattended hosts. Closing a CLI command or detaching from a terminal never stops the host.

Use one absolute data directory per host:

```sh
./shellfox --user-data /srv/shellfox-data host run --json
./shellfox --user-data /srv/shellfox-data host status --json
./shellfox --user-data /srv/shellfox-data host stop --yes --json
```

`host run --json` prints one readiness record (`mode`, `pid`, `startedAt`, `capabilities`) and keeps running. `SHELLFOX_USER_DATA` works like `--user-data`. Without either, the data root is `%APPDATA%\Shellfox`, `~/Library/Application Support/Shellfox` or `~/.config/Shellfox`, the same root the desktop app uses.

The host keeps its own state in `<data root>/rust-v1/host`. Files that Shellfox 0.9 wrote in the data root (`manager.sqlite3`, `remote.json`, `ssh-profiles.json`, `tailscale/`) are imported on the first start. See [updating from 0.9](#updating-from-09).

## Commands

```sh
shellfox session list --json
shellfox session create /srv/project --title Backend --json
shellfox session inspect SESSION_UUID --json
shellfox session settle SESSION_UUID
shellfox session stats --all
shellfox session delete SESSION_UUID --yes
shellfox terminals list [SESSION_UUID] --json
shellfox terminals create SESSION_UUID [--title T] [--profile P]
shellfox terminals write TAB_UUID --stdin < commands.txt
shellfox terminals attach TAB_UUID
shellfox settings read --json
shellfox api getSnapshot --json
shellfox --host build-box session list --json
shellfox cmd --register deploy ./deploy.sh
shellfox host keep-awake [on|off]
shellfox ssh [NAME]
shellfox version
```

`ss` and `sessions` are aliases for `session`, and `tm` is an alias for `terminals`. Globals (`--user-data`, `--json`, `--yes`, `--host`) can come before or after the command. Everything after `--` is literal. Every 0.9 command is available; [terminal commands](cli.md) describes them all.

JSON successes are `{"ok":true,"value":...}`. Failures are `{"ok":false,"error":{"code":"VALIDATION","message":"...","retryable":false}}`. Invalid arguments exit with 2 and failures with 1. Destructive commands need `--yes` in JSON or noninteractive mode.

Attach forwards input, output and resizes to the existing PTY. Ctrl+] detaches and leaves the shell running. Attach needs a TTY and rejects `--json`.

Human output uses the 0.9 tables and fields. A few records differ from 0.9; see [what differs from 0.9](cli.md#what-differs-from-09).

## Remote access without a browser on the server

```sh
shellfox network enable --hostname build-server --json
shellfox network login --json
shellfox network login --auth-key-stdin --json < private-auth-key
shellfox network login --auth-key-file /protected/auth-key --json
shellfox machines list --json
shellfox machines connect DEVICE_ID --json
shellfox approvals list --json
shellfox approvals approve DEVICE_ID --json
shellfox approvals revoke DEVICE_ID --yes --json
shellfox network disable --yes
```

Open the returned login URL on another device, or pass an untagged, non-ephemeral Tailscale auth key on stdin. Never put a key in argv, shell history or service files. The first connection from a device needs approval on the host, and you can approve it over SSH. Tailnet policy must allow TCP 52743. See [remote connections](remote-connections.md).

## Automation processes

```sh
shellfox run --cwd /srv/project -- node worker.mjs --task review
shellfox run --detach --json -- node worker.mjs
shellfox runs list --json
shellfox runs show RUN_UUID --json
shellfox runs stop RUN_UUID --yes
```

`run` starts an ordinary piped process in the local host. It forwards stdin and EOF, keeps stdout and stderr separate and returns the exit code. Foreground `run` rejects `--json`. Arguments after `--` are literal. Invoke a shell explicitly if you need shell parsing. `runs show` adds a read-only terminal view of recent output. Jobs end when their host stops.

A run's descendants stop with it: Windows holds them in a Job Object, Linux in a cgroup (or, without a systemd user manager, the run's own session), and macOS in the run's own session. In a session a program that starts a new session (`setsid`) can leave that boundary and keep running.

## Startup management

```sh
shellfox host service install
shellfox host service install --startup
shellfox host service uninstall --yes
```

Linux uses a systemd user service, macOS a LaunchAgent, and Windows a same-user limited Scheduled Task at login. On Windows, `--startup` registers a boot-time S4U task. It needs an elevated shell of the intended user and never runs as SYSTEM. Linux servers may need `loginctl enable-linger <user>`.

Unit, agent and task names are derived from the data directory the same way 0.9 did. Startup installed by 0.9 can be removed with this command, and the reverse also works. Definitions always point at the original installation, so they follow updates. Uninstall asks the host to stop first. The templates are in `service/`.

No startup job is installed by tests. Check reboot behavior on the target machine before relying on it.

## Updates

```sh
shellfox update --check --json
shellfox update --json
```

`--check` reads the latest stable GitHub release. `update` downloads the archive for this OS and architecture, checks its size and SHA-256 from the release metadata, validates the package and installs it beside the current one. Then it atomically points `headless-update.json` in the original installation at it. A missing asset or digest is an error. Remote `--host` is rejected.

Keep using the original launcher and service paths. The original installation forwards each start to the selected version. The running host and its terminals are left alone until you restart it through its startup manager, or with `host stop --yes` and `host run` using the same `--user-data`. Restarting ends its terminals and jobs. JSON returns `phase` (`current`, `available` or `installed`), `installation` and `restartRequired`. Updates serialize through `headless-update.lock` in the original installation.

Update as the same user that runs the host, with write access to the installation and its parent. Version directories are created next to the original. Keep them together and don't delete a version a host may still be using. To go back to the original version, stop the host and delete `headless-update.json`.

## Updating from 0.9

`shellfox update` in a 0.9.15 headless package installs this version like any other release. Nothing else changes for scripts or services:

- The 0.9 launcher and the units, agents and tasks it installed keep working. They start `bin/node app/main.cjs` in the selected version, which is the Rust CLI under that name.
- If the 0.9 host is still running, `host status` and `host stop --yes` still reach it. `host run` refuses to start next to it. Restart through the startup manager, or stop it and start again.
- On the first start, the host imports settings, sessions, SSH profiles, device approvals, the remote hostname and the Tailscale node identity from the 0.9 files in the data root, so paired phones keep working without a new login. The 0.9 files are not changed.
- Running terminals are not carried over. Saved sessions come back without their shells, as after any 0.9 restart.

Rollback works for 30 days: stop the host and delete `headless-update.json`, and the original 0.9 package starts on its untouched data. Changes made in 0.9 after a rollback are not imported again when you return to this version. After 30 days the host deletes the imported 0.9 files (`manager.sqlite3*`, `remote.json`, `ssh-profiles.json`, `tailscale/` and similar). That prevents a 0.9 host and this host from ever running with the same Tailscale identity. Unknown files and the `rust-v1` directory are kept.

Windows ARM64 has no headless archive in this release, so 0.9 installations there stay on 0.9.15.

## Ownership and closing

One process owns a data directory at a time, through an OS lock. A desktop opened on a data directory a headless host already owns attaches as a client, and closing it leaves the host running. `host stop --yes` stops the owning host and its terminals, including a desktop-owned host. A restarted host does not restore running processes.

## Building the package

The release workflow builds the archive next to the desktop installers with `node scripts/package-rust.mjs`, then runs `node scripts/package-rust-headless.mjs` to extract and smoke it with no display. The archive is written to `tmp/rust-installers/shellfox-headless-<version>-<platform>-<arch>.{zip,tar.gz}` with a `.sha256` file.

Files only 0.9 needs are `bin/node(.exe)` (a copy of `bin/shellfox-cli`), `runtime.json`, `app/` and `util/`. The 0.9 updater refuses archives without them, and a 0.9 installation re-checks them on every start. They can be dropped once 0.9 installations no longer need to update.
