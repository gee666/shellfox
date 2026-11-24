# Shellfox

Shellfox is a terminal manager. Saved sessions sit in the left sidebar, and each session holds real terminal tabs on the right. Shells run in a separate host process, so closing or crashing the window doesn't kill them. The same host serves the `shellfox` CLI, other Shellfox desktops and the phone app over an embedded Tailscale node.

Since 0.10.0 Shellfox is written in Rust. The desktop UI uses GPUI, and terminals use the official libghostty-vt from Ghostty. It replaces the Electron app of 0.9 and updates from it in place.

## Install

Download from [GitHub releases](https://github.com/gee666/shellfox/releases).

| Platform | Desktop | Headless (host and CLI only) |
| --- | --- | --- |
| Windows x64 | `ShellfoxSetup.exe`, or the portable ZIP | ZIP |
| Linux x64 and ARM64 | `shellfox_<version>_<arch>.deb`, or the portable `.tar.gz` | `.tar.gz` |
| macOS x64 and ARM64 | `Shellfox.app` in a DMG, or the ZIP | `.tar.gz` |
| Android | `shellfox-mobile-<version>-android.apk` | |

Linux packages need glibc 2.31 or newer (Ubuntu 20.04, Debian 11). Install the `.deb` as your normal user with `sudo apt install ./shellfox_*.deb`, and don't run Shellfox itself with sudo. See [Ubuntu and Debian](docs/ubuntu.md).

iOS builds come from CI and aren't in the release yet. Windows ARM64 has no build in 0.10.0.

The phone app is a remote client. It connects to a desktop or headless host and can't run shells of its own. See [Mobile](docs/mobile.md).

## Using it

- Click + to create a session in a folder, on this machine or a connected one. Selecting a session shows its tabs. A session with no open terminal (after a restart, say) opens a fresh shell; commands are never restored.
- The + in the tab strip adds another shell. Right-click it to pick a profile, such as PowerShell, Command Prompt, a WSL distribution or Administrator PowerShell on Windows.
- Settle marks a session as done without stopping anything. Delete closes its terminals and removes it. By default an inactive session settles after 24 hours and is deleted 30 days later. Pinned sessions are never settled or deleted automatically ([details](docs/database-retention.md#automatic-settling-and-deletion)).
- The folder icon opens Files beside the terminal: browse, edit, preview Markdown, SVG, HTML and images, upload and download. Clicking a file path printed in a terminal opens it there. See [Files](docs/files-api.md).
- The eye button keeps the computer awake while Shellfox runs.
- Settings has General, Terminals, Agents, Remote access and SSH pages.

Closing a tab ends its shell. Quitting Shellfox with live shells asks first, then stops the host and its shells. A window crash leaves the host and shells running, and the next launch reattaches.

## Where data lives

| OS | Data directory |
| --- | --- |
| Windows | `%APPDATA%\Shellfox\rust-v1\host` |
| Linux | `$XDG_CONFIG_HOME/Shellfox/rust-v1/host`, or `~/.config/Shellfox/rust-v1/host` |
| macOS | `~/Library/Application Support/Shellfox/rust-v1/host` |

It holds the session database, settings, SSH profiles and host keys, device approvals and the Tailscale node state. On Windows the Tailscale helper runs from `%LOCALAPPDATA%\Shellfox\helpers\<arch>\`. Terminal output isn't stored; scrollback lives in memory and ends with the host.

Use `--user-data <dir>` (CLI) or `--data-dir <dir>` (desktop) to run a separate instance.

## Updating from 0.9

Shellfox 0.9.15 offers 0.10.0 through its normal in-app update, and the headless `shellfox update` installs it like any other release. Installer names and package IDs are the same as 0.9, so the update replaces the Electron app in place.

On the first start, 0.10 imports from the 0.9 files in the parent `Shellfox` directory:

- settings and window position;
- saved sessions (without their shells, as after any 0.9 restart);
- SSH profiles and known host keys;
- device approvals and remote access preferences;
- the Tailscale node identity, copied, so paired machines and phones keep working without a new login.

Running terminals aren't carried over. Chromium's caches are deleted right after the import. The rest of the 0.9 data stays for 30 days, so you can go back to 0.9, and is then deleted. If 0.9 runs again within those 30 days, its data is kept.

Headless details, including rollback, are in [headless deployment](docs/headless.md#updating-from-09).

## Updates

The desktop checks GitHub releases at startup and every hour. It downloads a verified update in the background and installs it when you quit, or right away with Install and restart. `shellfox update --check` and `shellfox update` do the same from the CLI. The Android app updates itself from GitHub with a checksum and signing-certificate check. See [in-app updates](docs/self-update.md).

## CLI and headless host

The Windows installer and the `.deb` put a `shellfox` command on PATH; the headless archive has its own launcher. It talks to the running host, desktop or headless:

```sh
shellfox session list
shellfox session create .
shellfox terminals attach TAB_UUID
shellfox ssh prod-web
shellfox host run            # headless: run the host in the foreground
shellfox host service install
```

Add `--json` for automation. See the [CLI reference](docs/cli.md) and [headless deployment](docs/headless.md).

## Remote machines

Enable Remote access in Settings on both machines and sign in to Tailscale. No separate VPN app or SSH server is needed. Connect from one machine, then approve it on the other. An approved device can run commands and read and write files as the host's user, so approve only devices you trust. Each connected machine gets its own sidebar section. See [remote connections](docs/remote-connections.md).

## SSH

Settings > SSH stores SSH profiles and imports PuTTY sessions. `shellfox ssh` opens a picker, and `shellfox ssh NAME` connects directly, in any terminal. A new host key needs confirmation; a changed one is refused. See the [CLI reference](docs/cli.md).

## Development

You need Rust 1.97.1 (from `rust-toolchain.toml`), Node 24.22.1, pnpm, and Go 1.27.3 or newer for the Tailscale helper. Windows also needs Visual Studio C++ tools and the Windows SDK. Run on the target OS; there is no cross-compilation.

```sh
pnpm install --frozen-lockfile
node crates/ghostty-vt-sys/build-support/prepare.mjs verify
node scripts/gpui-experiment.mjs      # build and open the desktop on tmp/gpui-experiment
cargo test -p shellfox-cli
pnpm typecheck
```

The dev desktop uses its own data directory under `tmp/` and never touches an installed Shellfox. On Windows it uses one Tailscale helper, `tmp/dev-helper/shellfox-tailscale.exe`.

Installers come from `node scripts/package-rust.mjs`, written to `tmp/rust-installers/`. To publish, set `version` in package.json and push a matching tag. A tag with a suffix, such as `v0.10.0-beta.1`, publishes a pre-release. See [Rust packaging](docs/rust-packaging.md) and [Mobile](docs/mobile.md).

Never commit credentials, even revoked ones or test fixtures. Run `pnpm test:secrets` before committing.

The Electron sources under `src/` stay until 0.10.0 is accepted, as the reference for look and behavior. Project decisions are in [SHIP_DECISIONS.md](docs/SHIP_DECISIONS.md).
