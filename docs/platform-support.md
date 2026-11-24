# Platform support

## Release targets

The release workflow (`.github/workflows/release.yml`) builds each target natively. Nothing is cross-compiled.

| Target | Rust target | Desktop | Headless |
| --- | --- | --- | --- |
| Windows x64 | `x86_64-pc-windows-msvc` | `ShellfoxSetup.exe` (Squirrel), ZIP | ZIP |
| Linux x64 | `x86_64-unknown-linux-gnu` | `.deb`, `.tar.gz` | `.tar.gz` |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | `.deb`, `.tar.gz` | `.tar.gz` |
| macOS x64 | `x86_64-apple-darwin` | DMG, ZIP | `.tar.gz` |
| macOS ARM64 | `aarch64-apple-darwin` | DMG, ZIP | `.tar.gz` |
| Android | | APK | |

Windows ARM64 has no build in 0.10.0, so 0.9 installations there stay on 0.9.15. iOS builds run in `apple-platforms.yml` but aren't released yet. The phone apps are remote clients and run no shells of their own ([Mobile](mobile.md)). Linux means glibc, not musl or Alpine. macOS needs 13 or later ([macOS](macos.md)). Windows needs ConPTY, so Windows 10 1809 or later.

In-app updates work only for the Windows installer, the `.deb` and the macOS app. See [updates](self-update.md).

## Linux glibc floor

Linux desktop and headless packages need glibc 2.31 or newer: Ubuntu 20.04, Debian 11 and later. They are built in an Ubuntu 20.04 container, the deb declares `libc6 (>= 2.31)`, and packaging fails if `shellfox` or `shellfox-cli` needs a newer `GLIBC_` symbol (`scripts/package-rust-glibc.mjs`). Electron 0.9.15 needed glibc 2.25. Ubuntu 18.04 and Debian 10 ran it but can't install the Rust package. There, the 0.9.15 updater reports a failed install and keeps the old app.

## Terminals and cleanup

Every tab is its own PTY, owned by the host process. Closing a tab or stopping the host ends the tab's process tree. A restart brings back saved sessions, not running shells.

### Windows

Tabs use ConPTY. Each tab runs in a Windows Job that kills everything left in it when the tab closes ([shellfox-terminal](../crates/shellfox-terminal/README.md#ownership)). The default shell is PowerShell 7, then Windows PowerShell, then Command Prompt. Administrator PowerShell tabs go through a separate helper ([administrator terminals](administrator-terminals.md)).

WSL tabs start `wsl.exe` as the distribution's default user, which must be an ordinary account, not root. A distribution is offered only when it has `/bin/bash`, `/usr/bin/env` and Python 3 with working pidfds, and launching needs `/usr/bin/python3`. The Job holds the Windows `wsl.exe` process. Shellfox doesn't track the Linux processes inside the distribution. See [native/wsl](../native/wsl/README.md).

### Linux

Each process picks one containment mode at first use and logs it as `Terminal containment: cgroup|session (reason)`:

- **cgroup** when a cgroup-v2 systemd user manager is available. Each shell runs in its own transient scope (`shellfox-terminal-<id>.scope`, `OOMPolicy=continue`), and closing kills the whole cgroup at once.
- **session** otherwise: root without a user manager, containers, WSL, distributions without systemd, cgroup v1. The tab's controller leads its own session and sweeps it on close. A program that starts a new session can escape.

Both modes need pidfd support and give shells `oom_score_adj` 300, which makes a runaway shell the kernel's preferred OOM victim. In cgroup mode an OOM kill stays inside that tab's scope. A host started before its user manager stays in session mode until restarted. Details and limits are in [Unix PTY support](../crates/shellfox-terminal/UNIX.md). Automation runs use the same modes ([headless deployment](headless.md#automation-processes)).

### macOS

Each shell leads its own session. Closing sends HUP, TERM and then KILL to that session. macOS has no cgroup equivalent, so a program that starts a new session can escape ([Unix PTY support](../crates/shellfox-terminal/UNIX.md#macos)).

## OS shutdown

Logout, shutdown, SIGTERM, SIGHUP and Windows `WM_ENDSESSION` make the desktop quit without asking. A host the desktop started stops with its shells (`crates/shellfox-desktop/src/system_end.rs`). A host the desktop didn't start, such as a headless host, keeps running.
