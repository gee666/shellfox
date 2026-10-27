# Headless Shellfox

Shellfox has two distributions for Windows, macOS, and Linux:

- **Desktop** includes the Electron window and a host. Its CLI talks to the same sessions shown in the window.
- **Headless** includes the CLI, Node runtime, terminal host, native helpers, and embedded Tailscale. It contains no Electron, Chromium, React, or desktop assets and needs no display server.

Both use the same session service, database format, validated host API, and remote connection protocol. An interactive terminal, desktop window, or remote client is a view of a host's actual PTY, not a copy of a shell.

## Start a server

Extract the headless archive for your OS and architecture, then run its launcher:

```sh
./shellfox host run
```

Windows includes `shellfox.cmd` and `shellfox.ps1`. The package includes Node and Tailscale; neither needs a separate installation. Linux still needs a supported glibc system, Python 3 with the required process APIs, and ordinary shell/runtime dependencies. macOS uses its bundled native process helpers.

The foreground host runs until an authenticated stop request or termination signal. Run it under a startup manager for unattended use. Exiting an ordinary CLI command or detaching from a terminal does not stop the host.

For separate instances, use an absolute data directory consistently:

```sh
./shellfox --user-data /srv/shellfox-data host run --json
./shellfox --user-data /srv/shellfox-data host status --json
```

The first command prints one readiness JSON value and stays running. The second runs from another terminal. Do not run the server as root or Administrator just to host ordinary terminals.

## Use the same commands with the desktop

The installed desktop CLI accepts the same session, terminal, settings, remote-access, and JSON commands. A running desktop publishes the shared host API. These commands do not launch another window:

```sh
shellfox host status --json
shellfox session create /srv/project --title Backend --json
shellfox session list --json
shellfox settings read --json
shellfox api getSnapshot --json
```

Use OS-appropriate paths. Session and terminal IDs returned by the CLI are the same IDs the desktop uses. Changes made through one interface appear in the other.

A desktop-installed CLI can also start a foreground host with `shellfox host run` without opening a window. Its desktop distribution still contains Electron; use the separate headless archive when you do not want Electron installed at all.

`shellfox session create [path]` can launch the desktop distribution when used interactively without an existing host. JSON automation and the headless package never silently launch a desktop. Start a host explicitly before automation. Old top-level `start`, `ls`, `stats`, `settle`, and `tab` commands are no longer accepted.

## Ownership and closing

Exactly one process may own a data directory. An OS-held file lock is acquired before opening the database or initializing sessions. It is not a PID file, and its stable lock file is not deleted on normal shutdown. The ownership helper releases the kernel lock on EOF or process death. A host that loses its helper stops serving requests and shuts down rather than continuing unlocked.

- Closing a desktop-owned app ends its host and local terminals after the usual confirmation.
- A desktop opened while a headless host already owns the same data directory attaches as a client. Its title says `host client`. Closing that window leaves the host and terminals running.
- `shellfox host stop --yes` explicitly stops the owning host and its terminals, including a desktop-owned host. It does not require clicking a confirmation dialog.
- A stopped or rebooted host does not restore running processes. Saved session information is not a process checkpoint.

A desktop client never opens the headless host's database directly. It connects through the authenticated local API. Another OS user's data directory and credentials are not a way to bypass authentication; run clients with the intended host-user permissions.

## Remote access without a browser on the server

```sh
shellfox network enable --hostname build-server --json
shellfox network login --json
```

Open the returned login URL on another device. Alternatively, use an untagged, non-ephemeral Tailscale auth key through stdin or a protected file:

```sh
shellfox network login --auth-key-stdin --json
shellfox network login --auth-key-file /private/enrollment-key --json
```

Do not put the key directly in argv, shell history, service definitions, or environment files. Shellfox validates key-file ownership and permissions and does not save the enrollment key in application preferences.

```sh
shellfox machines list --json
shellfox machines connect DEVICE_ID --json
shellfox approvals list --json
shellfox approvals approve DEVICE_ID --json
shellfox approvals revoke DEVICE_ID --yes --json
```

The first remote connection requires approval on the host. It can be approved through that host's CLI over SSH; a desktop window is not required. The current Tailscale version supports the same user's devices in the same tailnet, not tagged nodes or cross-user sharing. Tailnet policies must permit TCP 52743. See [remote connections](remote-connections.md).

Use `--host DEVICE_ID_OR_UNIQUE_NAME` for remote session and terminal operations. Network, device-approval, and host lifecycle commands manage the local owning host and reject a remote selector rather than modifying the wrong machine.

## Interactive attachment and automation

```sh
shellfox terminals list --json
shellfox terminals attach TERMINAL_UUID
```

Attachment forwards terminal input, output, and size changes to the existing PTY. Ctrl+] detaches; it does not terminate the terminal. A lost transport does not replay uncertain input. Local terminal modes are restored on detachment, errors, EOF, and signals.

Interactive SSH and terminal attachment require a TTY and do not accept `--json`. Use terminal write and validated API requests for automation:

```sh
shellfox terminals write TERMINAL_UUID --stdin --json
shellfox api renameSession --input '{"sessionId":"SESSION_UUID","title":"Backend"}' --json
shellfox session delete SESSION_UUID --yes --json
```

JSON successes contain `ok: true` and a `value`. Failures have the form `{"ok":false,"error":{"code":"VALIDATION","message":"...","retryable":false}}` and a nonzero exit code. Do not automatically retry an uncertain mutation. A caller-supplied creation request ID can make deliberate creation retries idempotent.

Read-only command output is a bounded snapshot, not a permanent transcript. Use `session list --json`, `session stats SESSION_UUID --json`, and `session processes SESSION_UUID --json` for fresh process inspection. Use `--all` for aggregate metrics. Inside a local host terminal, omit the UUID to use its session context; an explicit UUID overrides that context. Destructive commands require `--yes` in noninteractive or JSON mode. See the complete [CLI reference](cli.md).

## Startup management

The packaged CLI includes:

```sh
shellfox host service install
shellfox host service uninstall --yes
```

Startup management uses systemd user services on Linux and LaunchAgents on macOS. Those are user services, not a claim that every machine will run them before login. Linux servers may need administrator-enabled user lingering. The package includes templates and deployment notes for administrator-managed setups.

On Windows the default is a limited, current-user scheduled task at login. `host service install --startup` registers an unattended startup task using S4U from an elevated shell of the intended user. It does not run as SYSTEM, save an account password, or pretend that Node implements the Windows Service Control Manager protocol. S4U does not provide Windows integrated-network credentials. Local shells and Tailscale use their own resources and node identity.

Installers and templates must be exercised on the target OS before relying on reboot behavior. No real startup job was installed on the development machine during verification.

## Packages and GitHub Actions

```sh
pnpm build:headless
pnpm package:headless
pnpm test:headless
```

Source builds require Node 24.22.1, the pinned pnpm version, Go 1.27.3 or newer, and the relevant native build tools. Matching Node-API prebuilds are staged separately from Electron's modules. Linux PTY source builds use checksum-verified Node headers. Downloads are verified before extraction or execution.

Headless output is under `tmp/headless/shellfox-headless-<platform>-<arch>/`; archives and SHA-256 sidecars are under `tmp/headless-artifacts/`. The package holds `bin/node`, `app/main.cjs`, private native runtime modules, and `resources/`. Its normal dependencies do not include Electron.

The release and preview workflows build both product families for Windows, macOS, and Linux. Existing architecture coverage is retained; there can be more than six archive files. Headless artifacts are named `shellfox-headless-<platform>-<arch>` and desktop artifacts `shellfox-desktop-<platform>-<arch>`. Desktop release asset names stay unchanged for updater compatibility. Headless jobs do not install a display server or download Electron.

Local Windows tests do not prove native macOS/Linux execution, reboot startup, signing/notarization, or authenticated two-device Tailscale connectivity. Those remain target-environment acceptance checks.
