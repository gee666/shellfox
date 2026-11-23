# Shipping decisions

Decisions for the Rust/GPUI/Ghostty migration, one line each. Read this before starting work.

## Goal

Ship Shellfox 0.10.0 on Rust, GPUI and libghostty-vt, replacing Electron 0.9.15 through the normal in-app update. Windows, Linux (desktop and headless) and Android come first. macOS and iOS are built and tested in CI.

## Working rules

- Build, run, test, fix. No source freezes, hash manifests or sign-off ceremonies.
- Every task fixes a user-visible defect or adds a missing required feature, and has a clear stop condition.
- Prefer existing code. No new protocols or subsystems without a decision recorded here.
- One review per risky change (auth, remote access, file writes). No review cycles on docs.
- Don't write proposal or review documents.
- Never touch the user's installed Shellfox or personal devices. Use isolated data dirs and your own emulators.
- Pushing this branch and running CI is allowed. Publishing a release needs the user.
- Windows development uses one Tailscale helper, `tmp/dev-helper/shellfox-tailscale.exe`, rebuilt in place. Test listeners bind 127.0.0.1, so the firewall asks once.

## Product

- Electron 0.9.15 is the reference for look and behavior. Match the intent; don't copy its bugs or slowness.
- The terminal engine is the official libghostty-vt 1.0.1, with no patches.
- The desktop UI is a client of a separate host process, `shellfox-cli --desktop-host` ([desktop-host.md](desktop-host.md)). A UI crash, or an OOM kill of one shell, must not kill the other shells or the UI. Logout, shutdown, SIGTERM and SIGHUP stop the host and its shells, as in 0.9.15.
- Approved devices get terminal control and full filesystem access for the host account, without elevation.
- Remote automation runs that aren't elevated are visible read-only to approved devices, like ordinary sessions.
- Remote Files reconnects an established peer by itself and reattaches only after checking the file scope and revision. It never saves on its own or replays a write.
- Local WSL Files runs an embedded Linux worker in the tab's distro as the tab's recorded non-root UID. A missing or different account is refused.
- Startup never refuses to launch because of the inherited ACL on our data dir. It tightens the ACL instead.
- On Windows the Tailscale helper runs from `%LOCALAPPDATA%\Shellfox\helpers\<arch>\`, so the firewall prompt appears once.
- Linux uses the native Rust PTY and supervisor. The legacy Linux Python preference is dropped on import; WSL keeps its guest Python check.
- Ghostty's own key encoder handles all input on desktop, the peer viewer and mobile. No custom key tables.
- Tests must cover: mobile soft keyboards whose autocorrect or suggestions replace typed text (Android and iOS); typing, Ctrl/Alt chords, arrows, Home/End/PgUp/PgDn, F-keys, paste and IME on desktop; Windows ConPTY with PSReadLine.
- Mobile keyboards keep suggestions, swipe and voice on, as in 0.9.15. An autocorrect replacement is sent as backspaces plus the new text, computed in Rust.
- Terminal cell metrics come from font metrics rounded to device pixels. The 8 px gutter stays.
- Sidebar rows and tabs use GPUI virtual lists.
- Layout shortcuts are GPUI actions with a keymap.
- Window bounds are stored in GPUI coordinates. Electron bounds are converted once, at import.
- Syntax highlighting cutoffs are byte limits, not UTF-16 units.
- No Chromium 1-2 px raster nudges. Controls share one style.
- Numeric fields are plain inputs without spin buttons. Up and Down step the value.
- The color picker is a compact theme-colored popup: SV square and hue slider, preview with HEX, RGB and HSL rows, then presets and 3 saved custom colors. Small font and inputs, no eyedropper.
- Windows keep-awake uses both SetThreadExecutionState and the 30 s mouse nudge. Don't change it.
- The Windows new-tab profile menu lists Command Prompt.
- HTML preview: Linux always shows the Source view with its notice, and the build has no WebKitGTK or GTK dependency. Windows and macOS keep the sandboxed webview and serve local assets through a wry custom protocol. Its look doesn't change.

## Update from 0.9.15

- The first Rust release is 0.10.0. Test builds are GitHub pre-releases named v0.10.0-beta.N. The whole refactor goes into the beta; quality over speed, no deadline.
- The first Rust start imports Electron's settings, window bounds, sessions, SSH profiles and host keys, approvals and remote preferences, and copies its Tailscale state.
- Chromium caches are deleted right after the import. Electron's own data is deleted 30 days later, unless Electron ran again in between.
- Headless ships as Rust in 0.10.0 and updates itself from the Node 0.9.15 package.
- Android is tested with a production-signed APK installed over the existing app.
- The Electron sources (src/main, src/renderer, src/preload, Forge config) stay as the reference and rollback path until the user accepts 0.10.0. Then they go in one cleanup with their tests, packaging scripts and dependencies.

## Open

- One Administrator PowerShell run needs the user's UAC approval.
- After the beta: a per-terminal holder process so shells survive a host crash. It needs a new protocol; the user decides.
