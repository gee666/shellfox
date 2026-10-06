# Shellfox on Ubuntu and Debian

## Install

Download the `.deb` for your architecture and install it as your normal desktop user:

```sh
sudo apt install ./shellfox_*.deb
shellfox
```

Do not launch Shellfox with sudo. Ubuntu 22.04, 24.04+ and desktop flavours use the same package; GNOME is not required. The package installs the Electron runtime libraries, Python 3 and the `/usr/bin/shellfox` launcher. It recommends `python3-nautilus` for GNOME Files.

```sh
shellfox start .
shellfox start ..
shellfox start "/home/me/a folder with spaces"
shellfox --help
```

Paths resolve against the caller's directory. Each call creates/selects one session with a default terminal and returns without waiting for the app to close. Symlinks are resolved by the launcher. A missing folder prints an error and returns exit 1. With no arguments, `shellfox` opens the app normally.

The launcher installed by the package is always available. A loose package or from-source build can install its owned launcher in `~/.local/bin` using Settings → Terminal command. If that directory is absent from PATH, add it or log out and in:

```sh
export PATH="$HOME/.local/bin:$PATH"
```

Add that line to your shell's startup file if needed. New Shellfox tabs receive the launcher directory immediately; already running shells are not modified.

## Right-click menus

In Settings → Explorer, enable **Add “Open in Shellfox” to the file manager right-click menu**. Shellfox detects installed file managers and installs per-user entries for every supported one:

- **GNOME Files / Nautilus:** single folder and background menu provider for Nautilus 3.0 and 4.0. For the top-level menu, install the optional binding:
  `sudo apt install python3-nautilus`.
  Without it, an executable fallback appears under Scripts → Open in Shellfox. Restart Files yourself or log out and in to load the extension. Shellfox never kills Files.
- **Nemo:** directory and background actions.
- **Dolphin:** KDE Frameworks 5/6 service-menu locations, with executable desktop files.
- **Thunar:** a custom action merged into uca.xml without removing other actions.
- **Caja:** Scripts → Open in Shellfox, using the selected folder or current local folder.

Only local file locations are passed as argv; no folder text is executable shell source. Disabling integration removes only Shellfox-owned entries. Existing foreign files/actions are not overwritten. Installed state is checked from the actual files and launcher, not just a saved switch.

## Python

Linux terminal cleanup uses Python's pidfd APIs to identify and safely close owned processes. Settings → Python lets you choose an **absolute executable Python 3 path**. Leave it empty for auto-detection: configured path, then `/usr/bin/python3`, then `python3` on PATH. Saving a path verifies executable access and actual pidfd signaling; a successful change refreshes shell availability without restarting the app.

If Python is missing, Settings says “Python 3 not found. Set its path in Settings → Python.” Install Python 3 or choose its path. Python 3.9+, readable /proc, a compatible Linux kernel and permitted pidfd operations are required. A kernel version alone is not proof. WSL profiles on Windows still resolve Python inside the guest; this setting affects local Linux only.

Closing a terminal stops and enumerates its verified owned session, holds pidfds, signals those handles and confirms exit. Inaccessible/elevated descendants, lost prerequisites or the member bound can block cleanup honestly. There is no numeric-PID fallback. Failed pre-commit cleanup resumes surviving processes it stopped. Existing running shells are not rewritten by a settings change.

## Sandbox

Only the UI process is sandboxed; terminals and agents run with your full user permissions. The package makes Chromium's chrome-sandbox helper root-owned mode 4755. On Ubuntu 24.04+ systems that restrict unprivileged user namespaces, its guarded AppArmor profile is **unconfined** and grants userns; it does not confine your shells or agents. Older systems skip that profile. Do not use a Chromium sandbox-bypass flag to hide configuration errors.

## Build from source

Use target-native Node 24, pnpm matching package.json and fresh Linux dependencies. Never copy Windows node_modules.

```sh
sudo apt install build-essential python3 curl fakeroot dpkg libgtk-3-0t64 libnss3 libxss1 libgbm1 libnotify4 libxtst6 libasound2t64 xdg-utils
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test:unit
pnpm build
pnpm make
sudo apt install ./tmp/packages/make/deb/*/shellfox_*.deb
```

On Ubuntu 22.04, use libgtk-3-0 and libasound2 instead of their t64 names. node-pty compiles against the pinned Electron on Linux. `scripts/ubuntu-stage.mjs` prepares an isolated source tree; native Linux filesystem builds are preferable to Windows-mounted node_modules.

A loose app is `tmp/packages/Shellfox-linux-<arch>/shellfox`. If testing a loose/dev app requires the setuid helper, configure only that build's chrome-sandbox with administrator approval; the installed deb already handles it. No sandbox bypass is added by Shellfox. Headless testing uses a graphical wrapper:

```sh
dbus-run-session -- xvfb-run -a pnpm test:storage
```

## Verification scope

Round 4 actually built and installed the Linux x64 deb in the existing WSL Debian 13 distro, opened its installed UI without a sandbox bypass under Xvfb, created real Bash terminals through cold/warm installed CLI calls, exercised Python settings, and imported/called Nautilus 4.0 with real GI. Ubuntu 22.04/24.04 physical desktops, AppArmor kernel-policy loading, and live Nemo/Dolphin/Thunar/Caja desktop menus were not run. See `tmp/round4-linux-report.md` for executed checks versus unit coverage. Squirrel and macOS/Linux ARM64 acceptance are separate work.
