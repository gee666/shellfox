# Verification

## Unit and native test boundary

`pnpm test:unit` uses [`vitest.config.ts`](../vitest.config.ts). It runs in-process desktop, headless, and mobile TypeScript contracts. Project `tmp/` files, loopback connections, and Vitest's own workers are allowed. App, helper, PTY, browser, and system-command launches are excluded or replaced with test doubles. A unit pass does not establish native ACL, ownership, or PTY behavior, or that no native addon was loaded.

[`tests/config/integration-registry.ts`](../tests/config/integration-registry.ts) lists the exact held files, platforms, and reasons. Unit config also excludes `*.os.test.{ts,tsx}` and `*.e2e.test.{ts,tsx}`. `pnpm test:integration:owned` reports `OWNED_INTEGRATION_HOLD`; [`vitest.integration.config.ts`](../vitest.integration.config.ts) likewise refuses before importing tests. There is no launcher for these cases in the current config. A built helper or an environment flag does not authorize a run.

A real run needs separate approval for the target OS, vetted binaries, fresh project `tmp/` fixtures, and an independent owner that retains original process handles or platform-equivalent ownership through launch, child and descendant shutdown, and confirmed exit. Ownership tests also need the unresolved Go3 original-root identity contract. Unknown exit or ownership stays on hold; do not recover it by PID or force cleanup. Results apply only to the OS and behavior actually exercised.

## Earlier review evidence

The host is Windows x64, Node 24.15.0 and Electron 44.5.1. The native Darwin supervisor and TypeScript backend/profile integration are implemented. The completed independent review fixed shell-birth tracking attestation and supervisor preparation/close races before the final packaging run; source stamps reject packages predating the reviewed sources. Native Darwin SDK/execution remains a separate verification gap.

| Check | Review result |
| --- | --- |
| Frozen dependency install | Pass |
| Current-source full typecheck | Pass |
| Current-source full unit suite | 30 files, 372 tests pass; Linux-kernel and Darwin-native opt-in cases skipped by default |
| Actual Linux kernel test through WSL Debian | Three tests pass, including the opted-in real PTY/pidfd safety test |
| Packaging configuration tests | Five pure Node tests pass; no native compiler/signing was invoked |
| Windows build/SQLite/PTY/package/packaged/startup/make | Pass on the stamped current sources, including ZIP and unsigned Squirrel creation; installer not run |
| macOS helper compilation/signing/native execution | Cannot run on this Windows host; native CI configured, not executed |
| Native Linux Electron package, ARM64, WSL UI, clean install | Not run |

Final logs use `tmp/embedded-packaging-final-*.log`; older `embedded-packaging-review` runs predate the latest integration/review. `tmp/package-source-stamp.json` identifies the reviewed artifact source tree, hash `d9450fcca009d513f4c65f564debb434d2dc27ff98e7dca70662bd085d2fd36c`. Build/package hashes remained stable, and native/startup smokes were rerun after make. The earlier pending-supervisor-integration blocker is superseded: compatible helper preflight now enables macOS profiles. This is not native macOS acceptance. The earlier raw-VT backend-smoke assertion and AttachConsole/list-PID close warning were fixed by the backend owner. The checked-in smoke now strips VT only in assertions; production still forwards raw VT. Windows uses the bundled ConPTY DLL/OpenConsole route. Do not retain those resolved failures as current blockers without a new failing run.

Forced Windows node-pty source compilation still needs Visual Studio Spectre-mitigated libraries, absent on this host. Default builds use the pinned target-native Node-API prebuild and validate it in Electron. No compiler flags were weakened or machine tools installed.

## What kernel evidence proves

The real pidfd and namespace regression is now in `src/main/terminal/unix-close.os.test.ts` and remains on HOLD. The unit counterpart checks source and command contracts. Neither those checks nor the earlier WSL kernel run establishes target-native Linux packaging, WSL renderer behavior, or Darwin supervision.

## Repeatable commands

Run on the target OS/architecture with an outer timeout. Keep scratch/userData under project `tmp/`. Do not run rebuilds, package/make or GUI suites concurrently.

```sh
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test:unit
pnpm test:packaging
pnpm build
pnpm test:storage
pnpm test:pty
pnpm package
pnpm test:packaged
pnpm test:packaged:startup
pnpm make
```

Allow about 300 seconds for install, 120 for typecheck/storage/PTY, 180 for unit/kernel tests and 600 for build/package/make. Linux Electron commands require a graphical session or Xvfb. Linux/WSL profiles additionally require actual Python/proc/pidfd preflight before launch. `pnpm rebuild` prepares native addons separately. Set `SHELLFOX_BUILD_FROM_SOURCE=1` to request source compilation with the documented toolchain.

macOS build/helper checks, separate from the held Vitest integration selection:

```sh
pnpm build:terminal-native
pnpm test:terminal-native
```

These need the real SDK/architecture, not a Windows stub. The real Darwin supervisor and PTY regression in `src/main/terminal/supervisor-native.os.test.ts` remains on HOLD. See [macOS build/signing](macos.md).

## Evidence levels

- Unit/config tests cover mocked orchestration, validation and packaging options, not desktop deployment or native signing.
- `test:storage` loads SQLite in Electron and tests migration/embedded metadata, not host-Node compatibility.
- `test:pty` uses the backend-owned native smoke as-is, external node-pty and isolated data. It exercises real profile spawn/output/resize/replay/exit/explicit close.
- `test:packaged` inspects ASAR/executable helpers and uses the actual packaged Electron for SQLite write/read and a no-profile native PTY fixture with observed output, dimensions and exit status. It additionally compiles the stamped real backend source into an external fixture, forces its production factory to resolve node-pty from that packaged ASAR, and checks profile readiness, replay, exit 7 and confirmed close. No fake factory is injected. It does not exercise production terminal IPC/UI.
- On macOS packaged tests also require resource-only helper paths, mode/architecture/signature and self-preflight, plus backend packaged path resolution and refusal to use development binaries when resources are missing.
- `test:packaged:startup` opens the actual production renderer/preload with isolated data, checks discovered safe-close profiles, and quits without creating shells. It is not terminal-input or quit-confirmation acceptance.

Native C helpers remain outside ASAR with executable modes. Packager signs them inside-out with unpacked native addons and spawn-helper before sealing the app; no post-sign mutation occurs. Preview macOS signing is ad-hoc. Optional Developer ID signing is configured, but certificate/notarization/Gatekeeper verification remains unexecuted.

No installer execution is part of these commands. Windows arm64 gets ZIP, not a Squirrel support claim. The manual six-target CI workflow has not run and publishes no release. Source files in a Windows package report do not prove Darwin execution.

## Legacy tests and artifacts

`build:test`, `test:e2e`, `test:windows`, `test:native`, `build:native` and the legacy GNOME smoke retain external-backend fixtures. Explicit legacy .NET commands still need SDK 8; default embedded builds do not. Historical registered-window survival/routing and helper checks are not embedded acceptance.

Old `0.1.0` Linux DEBs in `tmp/packages/make/` predate this rewrite. Do not distribute them as current artifacts. The stamped Windows artifacts must match the final reviewed source tree. Packaging checks both before and after native smokes; any later source correction requires a rebuild.

## Remaining native release checks

Use disposable users/VMs and obtain permission before installation or creating test shells.

- Compile/preflight/sign both Darwin helpers on x64 and arm64. Verify real shell job-control, supervisor authentication, guarded descendant cleanup, refusal/retry and missing/corrupt helper behavior.
- Exercise production preload/IPC, xterm parser replies, full-screen applications, resize, independent tabs and session/Settings retention on each target.
- Cancel the in-app quit modal with live shells/descendants using Cancel, Escape and its close button, then confirm. Verify all owned cleanup exits and restart shows saved sessions without command restoration.
- Simulate failed cleanup and SQLite exit-save failure. Ownership must remain authoritative; no silent replacement or stuck-frozen surviving groups.
- Check Linux/WSL preflight refusal before launch and restored-prerequisite recovery after launch. Test guest CWD translation and markers. Do not kill wsl.exe as a shortcut.
- Verify legacy sessions never adopts/watches/kills external shells.
- Test clean Windows/Linux installations, native ARM64, Windows shortcut lifecycle and macOS signing/quarantine/notarization policy.

Cleanup may stop only test-owned processes with current ownership evidence. Do not kill unrelated processes, terminal servers or WSL distributions, or suppress native refusals with numeric-PID fallback.

## Terminal screen restoration

The main process keeps a raw replay ring (256 KiB) and, per PTY, a headless xterm "screen mirror" (`src/main/terminal/screen-mirror.ts`) that parses the whole output stream in lockstep. When a view needs output the ring no longer holds (tab/session switch while a TUI keeps updating, a live burst, a reload), `attach` returns a serialized screen (`snapshot: true`: history, visible rows, SGR, cursor, scroll region, modes, active buffer) at the PTY's grid instead of a partial byte tail, which cannot reproduce a diff-rendering TUI. If no snapshot is possible the renderer replays the tail at the PTY grid and nudges the app to repaint (shrink by one row, restore).

Regression coverage: `screen-mirror.test.ts`, `backend-restore.test.ts`, `terminal-restore.integration.test.ts` (real backend, delivery, registry and a real xterm parser), `terminal-client.test.ts`, and the real-Electron `tests/e2e/terminal-tui-restore.spec.ts` (POSIX only):

```sh
node scripts/build.mjs --test && SHELLFOX_VERIFY_PROJECT=e2e node node_modules/@playwright/test/cli.js test --project=e2e terminal-tui-restore
```

## Settling and permanent deletion

Settling only changes status. Settled sessions remain in the session list with their live tabs and agents. Unsettling clears that status without relaunching terminals. Neither action requires confirmation.

`deleteSession` closes the session's owned terminals and removes its tabs, operations and per-tab metadata from SQLite. Deletion fails unless every terminal close is confirmed. The sidebar requires confirmation, and CLI `delete` requires inspection plus an explicit TTY answer.

Coverage includes `repository.electron-test.ts` via `pnpm test:storage`, `delete-session.test.ts`, `control-session.test.ts`, `control-server.test.ts`, `ipc.test.ts` and `SessionSidebar.test.tsx`. `tests/e2e/delete-session.spec.ts` uses the real embedded backend and checks that cancellation preserves terminal identity, confirmed deletion removes the session and the database stays clear after restart. `tests/e2e/live-control.spec.ts` covers CLI status changes and confirmed deletion through live terminal context.

```sh
node scripts/build.mjs --test && SHELLFOX_VERIFY_PROJECT=e2e node node_modules/@playwright/test/cli.js test --project=e2e delete-session
```
