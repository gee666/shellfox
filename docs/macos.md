# Shellfox on macOS

## Install

Releases have `Shellfox.app` for x64 and ARM64 in a DMG and in a ZIP (`Shellfox-darwin-<arch>-<version>.zip`, which the updater reads). The app needs macOS 13 or later. Drag it to `/Applications` or `~/Applications`.

Release builds are signed ad hoc, not with a Developer ID, and not notarized, so Gatekeeper treats them as coming from an unidentified developer.

When the app starts from one of those two folders, it writes a `shellfox` CLI launcher to `~/.local/bin/shellfox`. It replaces a launcher written by 0.9 but leaves any other file at that path alone. A copy started from the mounted DMG or from App Translocation installs nothing. Add `~/.local/bin` to `PATH` if your shell doesn't have it.

The in-app updater needs the app in a folder you can write to. See [updates](self-update.md).

## Terminals

Each tab is a PTY whose shell leads its own session. Closing the tab sends HUP, TERM and then KILL to that session's processes. A program that starts a new session can escape, because macOS has no cgroup equivalent. See [Unix PTY support](../crates/shellfox-terminal/UNIX.md#macos).

## Building

Build on a Mac of the target architecture with Xcode command-line tools and the macOS SDK, Rust 1.97.1, Node 24.22.1, pnpm and Go 1.27.3. Then:

```sh
node scripts/rust-package-prereqs.mjs
node scripts/package-rust.mjs
node scripts/package-rust-smoke.mjs
```

`package-rust.mjs` builds the app with deployment target 13.0, signs `shellfox-cli`, the Tailscale helper and the bundle, checks the result with `codesign --verify --deep --strict`, and writes the ZIP and DMG to `tmp/rust-installers/`. The smoke test mounts the DMG, checks that it holds the same app binary, and runs a real PTY.

Signing is ad hoc by default. To sign with a certificate already in a keychain, set `SHELLFOX_MAC_SIGN_IDENTITY` (and optionally `SHELLFOX_MAC_SIGN_KEYCHAIN`). That also turns on the hardened runtime and a secure timestamp. Nothing here imports certificates or notarizes.

The packages still contain two C helpers from `src/main/terminal/native/` in `resources/terminal-native/`. The Rust host doesn't run them. The headless package keeps them because a 0.9 installation checks for them when it updates (see [headless deployment](headless.md#building-the-package)).

CI builds and tests macOS in `.github/workflows/apple-platforms.yml` and packages releases in `release.yml`. The same workflow builds the iOS app; see [Mobile](mobile.md).
