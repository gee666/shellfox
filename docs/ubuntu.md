# Shellfox on Ubuntu and Debian

## Install

The `.deb` needs glibc 2.31 or newer: Ubuntu 20.04, Debian 11 and later, x64 (`amd64`) or ARM64. Install it as your normal user:

```sh
sudo apt install ./shellfox_<version>_<arch>.deb
```

Don't run Shellfox itself with sudo. Its shells, settings and Tailscale node belong to the user who starts it, and a root-run host gives approved remote devices root shells.

The package puts the app in `/usr/lib/shellfox/` (`shellfox` is the desktop, `shellfox-cli` the CLI and host) and adds a menu entry. `/usr/bin/shellfox` is the CLI. It hands `start`, `add`, `new`, `create`, `--new-session` and `--new-window` to the desktop in the background, as the 0.9 launcher did. `/usr/bin/shellfox-cli` links to the CLI directly.

```sh
shellfox session create .
shellfox session list
shellfox help
```

The window draws through X11 or Wayland with Vulkan. The package depends on fontconfig, FreeType, X11/XCB, xkbcommon, Wayland and `libvulkan1`, but you still need a working Vulkan driver. Python is no longer needed.

The portable `.tar.gz` has the same binaries without the package. It doesn't update itself.

## Updating

The installed app updates itself from GitHub releases. Installing needs `pkexec` and a graphical PolicyKit agent, and `shellfox update` uses `sudo` when run in a terminal. See [updates](self-update.md).

Upgrading from the Electron 0.9 package keeps the same package name and paths. The new `postinst` removes the AppArmor profile `/etc/apparmor.d/shellfox` that 0.9 added for Chromium's sandbox, but only if the file still carries Shellfox's marker line. The Rust app has no Chromium sandbox and needs no profile.

## Terminals

Each tab is a real PTY running your shell. Closing a tab, quitting or a host crash ends the tab's whole process tree, through a systemd user cgroup when one is available and otherwise through the tab's own session. See [platform support](platform-support.md#linux) for the modes and their limits.

## Headless servers

Servers without a display can use the headless `.tar.gz` instead. See [headless deployment](headless.md).

## Building from source

Build on the target machine, not from a Windows checkout. You need Rust 1.97.1, Node 24.22.1, pnpm and Go 1.27.3, plus the native build packages listed in [Rust packaging](rust-packaging.md#commands-and-prerequisites). `node scripts/package-rust.mjs` writes the `.deb` and `.tar.gz` to `tmp/rust-installers/`. See also [Development](../README.md#development).
