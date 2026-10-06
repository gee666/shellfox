# Verification

## Current review evidence

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

The opted-in `unix-close.test.ts` executes inside WSL Debian on this Windows host. It verifies HUP-ignoring background descendants are terminated through pidfds, unrelated PTYs/children survive, wrong birth and unknown orphan identities refuse, and exact proven orphan identities can close. Additional isolated PID/user/mount namespace checks cover SID reuse and post-commit interruption with survivors resumed.

This is actual Linux kernel evidence, not mock rows. It is also not a target-native Linux node-pty/Electron package, a WSL xterm/IPC flow or Darwin supervision test. The normal full unit run intentionally skips this opt-in case.

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

Actual Linux/WSL kernel regression:

```sh
SHELLFOX_UNIX_TREE_TEST=1 pnpm exec vitest run src/main/terminal/unix-close.test.ts
```

On Windows this selects Debian when available, or an explicitly selected `SHELLFOX_UNIX_TREE_DISTRO`. It can start a WSL distro and creates only test-owned processes. Private namespace checks require working `unshare` support; failure is an error, not a silent skip. The CI Linux jobs enable this case too. Disposable Ubuntu runners install util-linux, permit private namespaces for the isolated fixture, and configure root-owned mode-4755 Electron sandbox helpers. They do not disable Chromium sandboxing. These configured-runner results are not proof of default clean-machine policies.

macOS build/helper checks:

```sh
pnpm build:terminal-native
pnpm test:terminal-native
```

These need the real SDK/architecture, not a Windows stub. CI additionally enables `SHELLFOX_DARWIN_SUPERVISOR_NATIVE=1 pnpm exec vitest run src/main/terminal/supervisor-native.test.ts` on both Darwin runners. This native case is skipped by the normal unit suite. See [macOS build/signing](macos.md).

## Evidence levels

- Unit/config tests cover mocked orchestration, validation and packaging options, not desktop deployment or native signing.
- `test:storage` loads SQLite in Electron and tests migration/embedded metadata, not host-Node compatibility.
- `test:pty` uses the backend-owned native smoke as-is, external node-pty and isolated data. It exercises real profile spawn/output/resize/replay/exit/explicit close.
- `test:packaged` inspects ASAR/executable helpers and uses the actual packaged Electron for SQLite write/read and a no-profile native PTY fixture with observed output, dimensions and exit status. It additionally compiles the stamped real backend source into an external fixture, forces its production factory to resolve node-pty from that packaged ASAR, and checks profile readiness, replay, exit 7 and confirmed close. No fake factory is injected. It does not exercise production terminal IPC/UI.
- On macOS packaged tests also require resource-only helper paths, mode/architecture/signature and self-preflight, plus backend packaged path resolution and refusal to use development binaries when resources are missing.
- `test:packaged:startup` opens the actual production renderer/preload with isolated data, checks discovered safe-close profiles, and quits without creating shells. It is not terminal-input or native-quit-dialog acceptance.

Native C helpers remain outside ASAR with executable modes. Packager signs them inside-out with unpacked native addons and spawn-helper before sealing the app; no post-sign mutation occurs. Preview macOS signing is ad-hoc. Optional Developer ID signing is configured, but certificate/notarization/Gatekeeper verification remains unexecuted.

No installer execution is part of these commands. Windows arm64 gets ZIP, not a Squirrel support claim. The manual six-target CI workflow has not run and publishes no release. Source files in a Windows package report do not prove Darwin execution.

## Legacy tests and artifacts

`build:test`, `test:e2e`, `test:windows`, `test:native`, `build:native` and the legacy GNOME smoke retain external-backend fixtures. Explicit legacy .NET commands still need SDK 8; default embedded builds do not. Historical registered-window survival/routing and helper checks are not embedded acceptance.

Old `0.1.0` Linux DEBs in `tmp/packages/make/` predate this rewrite. Do not distribute them as current artifacts. The stamped Windows artifacts must match the final reviewed source tree. Packaging checks both before and after native smokes; any later source correction requires a rebuild.

## Remaining native release checks

Use disposable users/VMs and obtain permission before installation or creating test shells.

- Compile/preflight/sign both Darwin helpers on x64 and arm64. Verify real shell job-control, supervisor authentication, guarded descendant cleanup, refusal/retry and missing/corrupt helper behavior.
- Exercise production preload/IPC, xterm parser replies, full-screen applications, resize, independent tabs and session/Settings retention on each target.
- Cancel native quit with live shells/descendants, then confirm. Verify all owned cleanup exits and restart shows saved closed history without command restoration.
- Simulate failed cleanup and SQLite exit-save failure. Ownership must remain authoritative; no silent replacement or stuck-frozen surviving groups.
- Check Linux/WSL preflight refusal before launch and restored-prerequisite recovery after launch. Test guest CWD translation and markers. Do not kill wsl.exe as a shortcut.
- Verify legacy history never adopts/watches/kills external shells.
- Test clean Windows/Linux installations, native ARM64, Windows shortcut lifecycle and macOS signing/quarantine/notarization policy.

Cleanup may stop only test-owned processes with current ownership evidence. Do not kill unrelated processes, terminal servers or WSL distributions, or suppress native refusals with numeric-PID fallback.
