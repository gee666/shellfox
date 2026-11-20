# Shipping decisions

Active control file for the Rust/GPUI/Ghostty migration. Read it before starting or delegating work.

## Goal

Ship Shellfox on Rust + GPUI + libghostty-vt. The desktop app must match the Electron app exactly in look and functionality. Windows, Linux (desktop and headless) and Android are equal priorities. Mac/iOS follow after them.

## Working rules

- Normal engineering practice: build, run, test, fix. Skip custody receipts, source freezes, hash manifests and authorization scopes.
- Every task fixes a user-visible defect or adds a missing required feature, and has a clear stop condition.
- Prefer existing code. No new protocols, contracts or subsystems without a coordinator decision.
- One review per risky change (auth, remote access, file writes). No review cycles on docs.
- Don't write proposal or review documents. Record decisions here in one line each.
- Never touch the user's installed Shellfox or personal devices. Use isolated data dirs and agent-owned emulators.
- Each lane uses its own `CARGO_TARGET_DIR` under `tmp/<lane>/`. Delete it when the lane ends.

## Decisions

- The default terminal engine is the official libghostty-vt 1.0.1. The patched "native002" engine profile and its prototypes are experimental. They get removed unless the default path needs them.
- Approved devices get terminal control and full host-account filesystem access, with no elevation.
- The Rust host must serve the remote protocol the shipping mobile app speaks. Android must work against a Rust host before the Rust mobile terminal becomes the default.
- On Windows, the Tailscale helper runs from the stable path `%LOCALAPPDATA%\Shellfox\helpers\<arch>\`, so the firewall prompt appears only once.
- Dev/testing on Windows uses exactly one helper path, `tmp/dev-helper/shellfox-tailscale.exe`, rebuilt in place. Test listeners bind 127.0.0.1 only. This keeps the firewall prompt to one approval.
- Startup must never refuse to launch because of the inherited ACL on our own data dir. Tighten our dir's ACL instead.
- Electron is the pixel and UX reference. Compare against its screenshots for every UI change.
- Local WSL Files uses an embedded Linux worker in the tab's verified distro and non-root launch UID. The UID is recorded once with the tab. Missing or mismatched accounts are refused; Windows UNC operations and remote approvals are not fallbacks.
- Parity is checked strictly against `tmp/parity/inventory.md`, an exhaustive checklist built from the Electron source. Each item is marked done only after a side-by-side check in both apps, in the same state. The user reported missing behavior in the session path choice, path select, keybindings, the open terminal view and status icons.

## Decisions 2026-11-19

- Keep going until everything is done. Don't pause for status questions.
- Parity means intended behavior and look. Don't copy Electron's bugs or slowness.
- Remote automation replay: completed runs that aren't elevated are visible read-only to approved devices, the same eligibility as ordinary sessions. Record that provenance at launch.
- HTML preview: keep Linux's editable Source fallback and its notice where WebKitGTK cannot embed. Do not patch GPUI's window system for this now; native preview still uses the opaque sandbox.
- Remote Files: reconnect an established peer automatically and reattach only after checking the original file scope and revision. Never save automatically or replay a write.
- Android and iOS default to the native V1 Rust client and official Ghostty engine. The WebView terminal and unused TypeScript client are removed. ManagerV2/Native002 remains experimental.
- macOS/iOS: build and test in GitHub Actions on the experiment branch. Pushing the branch and running CI is allowed. Releases are not.
- Release version: the first Rust release is 0.10.0.
- Rust uses a native Linux PTY and supervisor even for imported GNOME Terminal settings. Drop the legacy Linux Python preference on import. WSL guest Python requirements stay in place.

## Status after harness crash (2026-11-19)

Done or verified:
- remote automation replay;
- local and remote WSL Files;
- `shellfox ssh` (russh);
- macOS PTY, with both macOS CI jobs green;
- Android native default terminal;
- Linux desktop parity sweep;
- version 0.10.0;
- Go test fixed paths.

Resumed lanes:
- pairing simplification, then one final review;
- P4 Files, Linux runtime;
- P5 Settings, last 6 rows;
- Android wrapped links;
- Apple CI for the native mobile default and macOS Files parity.

Open:
- P1: 7 Windows input rows, blocked by the user's open Windows Security (PickerHost) window;
- Admin PowerShell needs one user-approved UAC run;
- final installers rebuilt from final source.

## Current state (2026-11-18 assessment)

Windows GPUI works: terminal I/O, colors, selection, scrollback, tabs, sidebar, settings, files editor, and CLI automation.

Open defects:
1. ACL refusal at startup.
2. A stale Go helper blocks startup with no hint.
3. Close hangs when several sessions are open.
4. Cold `--new-session` leaves the tab inactive.
5. SSH is not composed in the app.
6. Legacy/mobile clients can't attach to Rust-hosted terminals.
7. Automatic updates are preference-only.
8. Parity gaps: title shows "Rust workspace", tabs named after the shell instead of "Shell N", tab status dot missing, second folder button missing, sidebar path lacks ellipsis, tab context menu lacks Rename/Move, font metrics and caret differ.

Linux: built on Nov 18 from an older snapshot. Needs a fresh build and smoke test.

Android: native V1 ran against a Rust host on an isolated x86_64 emulator over the Go test tailnet with adb reverse. The APK also builds for arm64-v8a. The iPad simulator passed native V1 pairing, PTY input and resize, listener outage and reattach, and Files save in Apple CI.
