# Ghostty and Rust baseline inventory

## Scope and state

This records the inventory worker's observations on `experiment/ghostty-rust`, starting at `c8cafb8`. It is not migration acceptance. Read `../IMPLEMENTATION_PLAN.md` for the architecture and milestone gates. The [GPUI terminal experiment guide](gpui-terminal-experiment.md) documents runnable commands, isolated paths and current limitations.

At the first inspection, only `IMPLEMENTATION_PLAN.md` was untracked. There was no Cargo workspace or Rust application. Other workers started the wrapper and desktop implementation during this inventory. Their work needs separate integration evidence.

The original `../shellfox` checkout remains on `main` at `c8cafb8`. Read-only `git status --short` and `git diff --stat` confirmed 20 tracked changed paths plus two untracked paths. The tracked diff has 222 insertions and 364 deletions. Changes cover `docs/mobile.md`, `mobile/App.tsx`, session/settings/files screens and tests, preferences, terminal input/sticky-modifier behavior, and deletion of `ModifierReleaseDelaySlider.tsx` and its test. `mobile/src/SwipeView.tsx` and its test are untracked there. None was copied, reverted, stashed, or committed. Do not treat this worktree's mobile baseline as the user's latest mobile work.

Read instructions were `C:/Users/user/.pi/agent/AGENTS.md`, repository `AGENTS.md`, and the unslop skill. No additional repository instruction files were found outside dependencies and scratch output. No `AGENTS.md` was changed.

## Milestone 0 checklist

- [x] Record git baseline and original-checkout divergence without changing it.
- [x] Identify the active host/terminal construction paths and representative parity tests below.
- [x] Record available toolchains and baseline TypeScript/unit results.
- [x] Verify Ghostty release tag, source-build Zig requirement, public Windows import-library route, and published GPUI version/checksum.
- [x] Verify retained Windows x64 ZIP and matching source/dependency tarball hashes against the authenticated upstream minisign manifest.
- [ ] Verify any additional target artifacts before use. Windows arm64/Apple artifact and other-platform native-build acceptance remain open.
- [ ] Audit the complete resolved Rust dependency/license graph and bundled Ghostty dependency notices.
- [ ] Validate desktop minimum OS/graphics requirements and reconcile iOS project/React Native deployment minima.
- [x] Establish and exercise isolated experiment paths and reviewed owned-process cleanup for the Windows x64 slice.
- [ ] Complete equivalent ownership/data-isolation acceptance on remaining targets and application integrations.
- [ ] Execute platform-native acceptance across Windows/macOS/Linux desktop and headless, Android, and iOS.
- [ ] Expand the feature groups below into per-behavior migration signoff as each subsystem changes.

Milestone 0 is partial. Mapping source and tests is not proof that the baseline passes on every target.

## Active feature and parity checklist

Every row is pending Rust replacement acceptance. User explicitly requires "interface should be pixel-perfect exactly the same as electron app now. And functionality." Match all committed-worktree Electron UI states and full behavior, not approximate/simplified/reduced UX; deviations need user approval. See [controlled goldens and independent interaction acceptance](electron-gpui-visual-parity.md). Earlier GPUI 18/25/native proofs are progress, not pixel-perfect acceptance. Existing tests remain authoritative.

| Behavior to preserve | Active implementation | Representative existing verification | Migration stage |
| --- | --- | --- | --- |
| Pixel-perfect current Electron desktop, every state and full interaction/accessibility parity | Committed worktree `src/renderer/`, preload/main/native chrome; isolated actual Electron goldens | Same OS/DPI/fonts/theme/window/data; paired whole-window pixel/geometry/font/color/icon comparisons and separate interaction matrix; native OS proof, no required-content masking | 3 and final acceptance |
| Shared desktop/headless host, resource ownership and authenticated RPC | `src/host/runtime.ts`, `resources.ts`, `ownership.ts`, `rpc.ts`, `src/main/attached-host-ipc.ts` | `src/host/ownership*.test.ts`, `rpc.test.ts`, `src/main/attached-host-ipc.test.ts`, `tests/e2e/headless-host.spec.ts` | 2 |
| Exactly one real PTY per terminal, launch/input/resize/exit | `src/main/terminal/backend.ts`, `service.ts`, `native-input.ts`, `profiles.ts` | `backend.test.ts`, `backend-lifecycle.test.ts`, `native-input.test.ts`, `scripts/pty-test.mjs` | 1 and 2 |
| Authoritative query replies, title/CWD, ANSI recovery and parser continuation | `src/main/terminal/screen-mirror.ts`, `cwd-hook.ts`, `src/shared/terminal-compat.ts` | `backend-queries.test.ts`, `backend-restore.test.ts`, `screen-mirror*.test.ts`, `src/host/terminal-compat.test.ts` | 1 and 2 |
| Controlling view, takeover/epochs, sequence/ACK credit/replay/resize ordering | `src/main/terminal/control.ts`, `delivery.ts`, `src/shared/terminal-view.ts`, `src/renderer/terminal-client.ts` | `control*.test.ts`, `delivery.test.ts`, `src/renderer/terminal-delivery.integration.test.ts`, `mobile/src/client/exclusive-host.test.ts` | 2 through 4 |
| Exact process identities, failed-cleanup retention, no numeric-PID fallback | `tracking.ts`, `unix-close.ts`, `darwin-supervisor.ts`, `windows-elevated.ts`, under `src/main/terminal/` | `unix-close.test.ts`, `shutdown.test.ts`, `backend-lifecycle.test.ts`, `windows-elevated.test.ts`, `scripts/linux-session-smoke.mjs` | 1 and 2 |
| Normal/elevated Windows, WSL, Linux scope/pidfd cleanup, Darwin supervisor | `src/main/terminal/profiles.ts`, `linux-scope.ts`, native Darwin C helpers, `native/Shellfox.ElevatedTerminal/` | `administrator*.test.ts`, `nested-wsl-marker.test.ts`, `linux-scope.test.ts`, `supervisor-native.test.ts`, `tests/e2e/terminal-wsl-*.spec.ts` | 1 and 2 |
| SQLite/session/settings retention and upgrade | `src/main/repository.ts`, `user-data-upgrade.ts`, `src/main/terminal/service.ts` | `repository.electron-test.ts`, `user-data-upgrade.test.ts`, `tests/e2e/database-retention.spec.ts`, `migration.spec.ts` | 2 |
| Terminal rendering, tabs, scrolling, selection, links, clipboard and keyboard | `src/renderer/terminal-surface.ts`, `terminal-links.ts`, `terminal-queries.ts`, `TerminalWorkspace.tsx` | `terminal-links*.test.ts`, `terminal-queries.test.ts`, `terminal-restore.integration.test.ts`, `tests/e2e/terminal-*.spec.ts` | 1 and 3 |
| Mobile keyboard/IME, deletion/composition, sticky modifiers and reconnect | `mobile/src/terminal/`, `mobile/src/client/`, `mobile/src/network-recovery.ts` | `native-text-input.test.ts`, `native-text-edits.test.ts`, `TerminalView.test.tsx`, `terminal-page.test.mjs`, native IME/e2e scripts | 4 |
| Session list/settings/themes/status, agent detection, multi-window behavior | `src/renderer/`, `src/main/terminal/activity.ts`, `tracking.ts`, `src/main/window-state.ts` | `src/renderer/App*.test.tsx`, `SessionSidebar*.test.tsx`, `tracking*.test.ts`, `tests/e2e/terminal-tabs-window.spec.ts` | 2 and 3 |
| Files, previews/editing, transfers, path restrictions and staging races | `src/main/files/`, `src/renderer/files/`, `src/shared/file-*.ts`, `mobile/src/files.ts` | `src/main/files/*.test.ts`, `src/main/remote/upload-*.test.ts`, `tests/e2e/files*.spec.ts`, `file-transfers.spec.ts` | 2 through 4 |
| Remote pairing/approval/revocation, permissions and transport ownership | `src/main/remote/`, `native/tailscale/`, `native/mobile/` | `approvals.test.ts`, `permissions.test.ts`, `view-permissions.test.ts`, Go tests, `tests/e2e/connection-permissions.spec.ts` | 2 and 4 |
| CLI commands/attachment and ordinary-terminal query suppression | `src/cli/`, `src/main/control-*.ts`, `src/host/cli-binding.ts` | `src/cli/*.test.ts`, `tests/e2e/cli*.spec.ts`, `scripts/cli-host-smoke.mjs` | 2 |
| Automation processes, raw pipes/backpressure and descendant lifecycle | `src/host/automation*.ts` | `automation.test.ts`, `automation-lifecycle.test.ts`, `src/cli/automation.test.ts` | 2 |
| SSH credentials/known-hosts/PTYs | `src/main/ssh/` | `keys.test.ts`, `known-hosts.test.ts`, `pty.test.ts`, `client.e2e.test.ts` | 2 and 3 |
| Keep-awake, Explorer/desktop launchers, shortcuts, notifications/window lifecycle | `src/main/keep-awake.ts`, `src/main/platform/`, `src/main/window-*.ts`, preload bridges | `keep-awake*.test.ts`, `src/main/platform/*.test.ts`, `tests/e2e/application-menu.spec.ts`, `quit-confirmation.spec.ts` | 3 |
| Desktop/headless/mobile updates and target-native packaging | `src/main/update/`, `src/cli/update*.ts`, `mobile/src/updates/`, `scripts/*package*.mjs`, `.github/workflows/` | `pnpm test:packaging`, packaged smoke scripts, `src/cli/update*.test.ts`, `mobile/src/updates/*.test.ts` | 5 |

`src/host/resources.ts` constructs `PtyBackend` and `EmbeddedSessionService` for both desktop and headless. Desktop currently uses xterm 6.0.0, while `screen-mirror.ts` uses `@xterm/headless` and `@xterm/addon-serialize` for ANSI recovery. Android/iOS remain remote-only React Native clients with a WebView terminal.

`native/README.md` describes retired external-window behavior. `scripts/native.mjs` still tests/publishes `Shellfox.Native` or `Shellfox.Linux`, so its reachability is historical/test tooling, not proof of current terminal behavior. The active Windows helper is built by `scripts/terminal-native.mjs` from `native/Shellfox.ElevatedTerminal/`. Do not port old window-routing code into the new terminal.

## Toolchains and platform constraints

Inventory ran on Windows x64 through Git Bash, MSYS_NT-10.0-26300. Initial PATH inspection found Node 24.15.0, pnpm 12.9.1, default Rust/Cargo 1.85.1, .NET SDK 8.0.204, Ninja 1.11.1, MSVC build tools 14.35.32215, Java 17 and Android adb. Zig, Go, CMake, clang and GCC were absent from PATH. Toolchain inventory is not a compile test.

`rustc +1.97.1 --version` returned 1.97.1 and `rustc +stable --version` returned 1.99.1. Installed default targets included `x86_64-pc-windows-msvc`, `x86_64-apple-darwin`, and `wasm32-unknown-unknown`; an installed target does not provide an Apple SDK or native acceptance. Prefer the worktree's explicit 1.97.1 toolchain over changing the global default. Current workers selected `gpui = "=0.2.2"` and `portable-pty = "=0.9.0"`; neither selection proves lifecycle safety.

Baseline CI pins Node 24.22.1 and Go 1.27.3. Local Node differs. Windows native source builds need the .NET SDK and package restore access. Go can be selected with `SHELLFOX_GO`; it is needed for the existing Tailscale permissions/ownership helpers even when a test makes no remote connection.

Existing target configuration covers Windows/Linux/macOS x64 and arm64, plus Android/iOS. `docs/platform-support.md` explicitly distinguishes configured builds from native launch/cleanup evidence. Linux is a glibc desktop target and existing packages target Ubuntu 24.04. Headless packaging states Windows 10+ and macOS 13+. These declarations do not establish the new GPUI GPU/driver or OS minimums.

Android declares minSdk 24, target/compile SDK 36 and NDK 27.1.12297006 in `mobile/android/build.gradle`. iOS project deployment targets are 15.1, but `mobile/ios/Podfile` asks React Native's `min_ios_version_supported`; resolve the effective value with the pinned RN 0.88.0 toolchain on macOS. Do not lower deployment targets to Ghostty's library-only iOS 13 minimum. Xcode/macOS and real Linux/native mobile runtime checks were not available in this inventory.

## Upstream dependency evidence

Read pinned upstream files, not only the moving main branches:

- `git ls-remote https://github.com/ghostty-org/ghostty.git 'refs/tags/vt-1.0.1*'` returned annotated tag `e9dc690b08bde7e2ec9c7aff8a18a85c9cade46d`, peeled commit `108cacb17ed8990840cd14abfce43d8ed8503ca1`.
- At that commit, `build.zig.zon` requires Zig 0.17.0. Packaging requires the same major/minor, allowing newer 0.17 patch releases. No Zig compiler was run in inventory.
- `PACKAGING-LIBGHOSTTY.md` recommends official source/dependency tarballs, not GitHub-generated archives. Git checkouts need `-Demit-lib-vt`; the official source tarball includes `LIB_VERSION`.
- Windows GNU-ABI release DLLs include `ghostty-vt.lib`. Upstream explicitly supports MSVC consumers through that import library and tests CMake examples. This is the preferred Windows route, not a known MSVC incompatibility. Do not assume GNU static archives work without a separate proof.
- Official assets and `SHA256SUMS` are under `https://release.files.ghostty.org/libghostty-vt/1.0.1/`. The fetched manifest lists source SHA-256 `333dfce97641ef9b536bb9e08907ad86abaaada50e8f7417b70d9284adef37aa`, dependencies `91a4e10960a52bf5bf4b15aa2a2cddaff8a89d6cd903d3055bda4fd17de3356d`, Windows x64 ZIP `8e2eb9e1822e85d786b456c8beb86f49d09c318c91f0c2913f91e7ffa12cda29`, and Windows arm64 ZIP `cc61da5f968ea0b08639bdf019c6b7da46bad1d985774fd8a2de57af32862892`.
- After integration, inventory ran `node crates/ghostty-vt-sys/build-support/prepare.mjs verify` and `node --test crates/ghostty-vt-sys/build-support/prepare.test.mjs` within a 60-second outer timeout. Both passed; all six tests passed. The retained Windows x64 ZIP and source/dependency tarballs match their pinned hashes and the authenticated upstream minisign checksum manifest. The tests also reject corrupted archives and tampered signatures/trusted comments, enforce exact Zig 0.17.0, and check offline recipes/target options. Windows arm64 and Apple artifacts were not part of this verification. No source build or other-target runtime is implied. Upstream minisign public key is `RWQlAjJC23149WL2sEpT/l0QKy7hMIFhYdQOFy0Z7z7PbneUgvlsnYcV`.
- Independent final review, relayed by the integration lead, found no blocking wrapper issues after the poisoned-state fix. `cargo test --locked -p shellfox-ghostty-vt --test terminal` passed six real-library integration tests; `cargo clippy --locked -p shellfox-ghostty-vt --all-targets -- -D warnings` and `pnpm typecheck` passed. ABI, ownership, thread restrictions and cleanup were reviewed. An independent 200,000-request device-attributes storm confirmed persistent `Err(-6)` resource-limit failure for feed/render/reply-drain/key/paste operations after overflow. Reviewer scratch artifacts were cleaned. This proves the reviewed Windows GNU DLL/import-library route under Rust 1.97.1 MSVC, not native AltGr/IME, contained PTYs, other OS execution, or allocation-failure injection. These are reviewer results, not inventory's own execution or application acceptance.
- `https://index.crates.io/gp/ui/gpui` lists published 0.2.2 as non-yanked, checksum `979b45cfa6ec723b6f42330915a1b3769b930d02b2d505f9697f8ca602bee707`. Its index record does not declare a Rust minimum. Keep an exact requirement and committed Cargo lockfile, then verify the resolved graph with the selected compiler.
- Current Zed HEAD `dc93abce2fa6c0bf349d425310db4884aa953467` also labels GPUI 0.2.2 but has a different workspace/API and Rust 1.98.1 toolchain. Do not use its examples as if they were the published crate. Published-source API inspection is required.
- Ghostty is MIT; the inspected GPUI manifest is Apache-2.0. Retain license texts and audit the resolved crate graph and Ghostty `deps/libghostty-vt-third-party.json`. This is not a completed license audit.

## Baseline verification

These are historical runs. [Later attributed results](ghostty-rust-worklog.md) include Juan's resolved helper prerequisites and Salahuddin's subsequent full `pnpm test:unit` pass, 338 files/4039 tests with 59 skips. The file-write ordering flake remains documented without a fix or hidden waiver. Do not merge run counts or repeat full/typecheck runs to reconfirm the handoffs.

All shell invocations had explicit outer timeouts. Tests used their existing isolated fixtures, not installed application data. Typechecking generates the existing ignored mobile terminal HTML through the established script.

| Command | Outer timeout | Observed result |
| --- | --- | --- |
| `pnpm typecheck` | 150 seconds | Exit 0, desktop and mobile typechecks passed in 11 seconds; mobile terminal HTML generated |
| `pnpm test:unit` | 150 seconds | Timed out; already reported host/RPC/CLI prerequisite failures; no suite completion claim |
| `pnpm test:unit --reporter=dot --no-color` | 540 seconds | Exit 1 after 206.85 seconds; 323 passed, 14 failed, 3 skipped files; 3953 passed, 60 failed, 80 skipped tests |
| Focused terminal command below | 100 seconds | Exit 0; 15 files and 194 tests passed in 9.06 seconds; jsdom reported missing canvas context, not an assertion failure |
| `pnpm exec vitest run src/host/automation.test.ts src/host/rpc.test.ts --reporter=dot --no-color` | 80 seconds | Exit 1; 19 failed, 1 passed, 3 skipped tests |
| `pnpm test:native` | 180 seconds | Exit 0 in 12.5 seconds; 79 .NET tests passed, then all 5 Node pipe-run tests skipped because the active helper was not built |

The inventory worker's completed full run was launched before the instruction to avoid further full reruns without prerequisites. Inventory did not rerun it again. That run's failures report the unavailable Windows private-permissions helper or missing `tmp/terminal-native/win-x64/Shellfox.ElevatedTerminal.exe`. `src/main/control-endpoint.ts` invokes `hostResourcePaths().tailscale` with `--private-permissions`; tests must not bypass this check to turn the baseline green. Juan later built and retained both helpers and reports the scoped passes in the worklog. This inventory worker did not execute those builds; the old run is not evidence of their current unavailability.

A separate independent reviewer full-suite run failed with 15 failed, 321 passed, 3 skipped files; 61 failed, 3947 passed, 80 skipped tests; and one unexpected Vitest worker exit. Most failures concerned control-endpoint permissions-helper availability. `src/main/files/service.test.ts` additionally expected `first` but received `second`. Juan later reproduced a flaky invocation-order assertion: path resolution precedes queue admission, either write can win, one succeeds and one conflicts, and unauthorized mutation is denied. No fix was made; the unexpected worker exit remains unverified. See the worklog for scope. Do not combine run counts, infer every failure was prerequisite-related, or rerun the full suite again in this pass.

Exact focused terminal command:

```sh
pnpm exec vitest run \
  src/main/terminal/backend.test.ts \
  src/main/terminal/backend-lifecycle.test.ts \
  src/main/terminal/backend-queries.test.ts \
  src/main/terminal/backend-restore.test.ts \
  src/main/terminal/control.test.ts \
  src/main/terminal/delivery.test.ts \
  src/main/terminal/screen-mirror.test.ts \
  src/main/terminal/screen-mirror-lifecycle.test.ts \
  src/renderer/terminal-client.test.ts \
  src/renderer/terminal-queries.test.ts \
  src/renderer/terminal-delivery.integration.test.ts \
  src/renderer/terminal-restore.integration.test.ts \
  mobile/src/terminal/native-text-input.test.ts \
  mobile/src/terminal/native-text-edits.test.ts \
  mobile/src/client/terminal-load.test.ts --reporter=dot --no-color
```

`pnpm test:native` restored/built the existing .NET test project successfully and passed 79 tests. The script then skipped all five Node pipe-run tests because `tmp/terminal-native/win-x64/Shellfox.ElevatedTerminal.exe` was absent. It covers historical .NET projects too and does not replace active-helper/terminal checks or prove contained shell cleanup. Inspection confirmed isolated test registry keys, scratch paths under root `tmp/`, and only test-owned process use. Disposable native fixture directories were cleaned; the generated build cache and `tmp/reports/native/native.trx` remain verification outputs. Real PTY/storage/GUI/packaged checks, Go helper tests and mobile native tests were not run by inventory. No lint script is configured in the baseline `package.json`. No release-build benchmark was run.

## Wrapper/build integration handoff

The build worker reports `cargo test --locked --offline -p shellfox-ghostty-vt -p shellfox-ghostty-vt-sys --all-targets` passed 17 tests, six wrapper/ABI integration tests and 11 build-support tests. Scoped wrapper/sys Clippy with warnings denied passed, as did six release wrapper integration tests. Release ABI loading is not application performance evidence.

Verified Windows x64/source/dependency archives are intentionally retained, 16.46 MiB total as reported by the build worker. See [the build guide](../crates/ghostty-vt-sys/README.md) for hashes, deployment requirements, provenance and retained notices. These are reproducible build inputs rather than temporary artifacts. The source recipe pins exact Zig 0.17.0; no source build ran. Default Android recipe API 26 needs reconciliation with the existing app minSdk 24; no change or removal of minSdk 24 support is authorized.

`.github/workflows/ghostty-experiment.yml` configures Windows x86_64 MSVC checks only. It has not run remotely. Other-platform CI/source-build acceptance remains open. No release was triggered. Final local workspace formatting and Clippy passed as recorded below.

## Final Windows slice evidence

Final independent review found no blocking cleanup/query defect in the isolated Windows x64 slice. These worker/reviewer results were relayed by the integration lead, not executed by inventory. Milestones 0 and 1 remain partial.

| Check | Reported result |
| --- | --- |
| `cargo check -p shellfox-desktop --locked` and `cargo build -p shellfox-desktop --locked` | Passed and linked the debug GPUI executable under Rust 1.97.1 MSVC |
| Native fixture launch/close | Worker observed an owned HWND titled `Shellfox GPUI experiment`; normal close exited within 10 seconds without forced kill |
| Native `--shell` launch/close | Worker observed GPUI and owned PTY together and normal close without forced kill |
| `node scripts/gpui-experiment.mjs smoke` | Worker and independent reviewer passed real system cmd-to-PTY-to-Ghostty marker-row output, encoded paste/Enter, resize to 100x30 and Job cleanup |
| `cargo test -p shellfox-desktop --locked` | Latest independent run passed 11 desktop tests |
| `cargo test -p shellfox-terminal --all-targets --features test-support --locked` | Final worker run passed 2 unit tests and 10 integration tests; independent review confirmed the 10 Windows lifecycle tests, including missing-GO timeout |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Final independent run passed |
| `cargo fmt --all -- --check` | Final independent run passed |

The separate wrapper/sys all-targets command passed 17 tests as recorded above; do not combine those counts. Final desktop/lifecycle counts supersede earlier 9/10 desktop and 9 lifecycle handoffs. Worker all-targets Clippy also passed with terminal `test-support` enabled. The linked build, final smoke, launcher syntax and diff checks passed. Authored desktop files are at most 338 lines and PTY files at most 234. Worker scratch was cleaned. `pnpm typecheck` passed; the full suite was not green at this slice. Salahuddin later reports the separate full pass in the worklog. No existing `src/`, `mobile/`, `native/`, package manifests, pnpm lockfile or instruction files were edited in the slice.

The Windows path uses the exact trusted supervisor executable. Shell creation cannot proceed before the original supervisor handle has been assigned to the non-breakaway kill-on-close Job, exact membership has been checked and GO released. READY gates input. Shutdown verifies root exit and an empty Job; failed cleanup retains owners and disables input. Missing GO cannot spawn a command. The preserved portable-pty source has only the documented cursor-inheritance patch, not a numeric-PID cleanup fallback.

The reviewed `Active`/`Draining`/`Closed` phases stop new input on natural exit and feed final output through EOF while discarding replies to retired writers. Polling continues in Active and Draining until delayed Job accounting and console closure finish. Regression coverage includes 20 chunks of 4096 bytes then DSR/DA/final marker without retired writes, and root exit on tick 1 with Job completion only on tick 3. All reviewer scratch was cleaned.

Read [the experiment guide](gpui-terminal-experiment.md), including the direct executable commands and known limitations. From the repository root:

```sh
node scripts/gpui-experiment.mjs fixture
node scripts/gpui-experiment.mjs shell
node scripts/gpui-experiment.mjs smoke
```

The mode defaults to fixture, which starts no PTY. Add `--release` to select a release build. Each launcher invocation creates a new `tmp/gpui-experiment/run-PID-TIMESTAMP` directory. Direct executable use requires absolute `--data-dir`. The DLL and notices are deployed beside the executable without PATH changes. The smoke route does not initialize GPUI but still links desktop dependencies; it is not a migrated headless distribution. Isolation is not a filesystem sandbox, and experiment run directories are not automatically removed.

The vendored unused inherit-cursor constant warning and `proc-macro-error2` future-incompatibility notice are acknowledged, not suppressed in Shellfox code. Full dependency license/security clearance remains open.

Unix PTYs remain gated before allocation. Windows arm64, Android/iOS and the new headless host are not accepted or migrated. Native IME/AltGr/dead-key/Option composition, enhanced printable-key metadata and focus ordering remain unverified. Selection/copy, links, mouse reporting and accessibility are missing. Per-cell glyph painting still needs optimized batching/shaping. No representative full-screen TUI, backlog-plus-resize ordering, DPI/GPU matrix, sustained-output response or release performance acceptance has been established. The unchanged production clients remain active. Juan's later TypeScript repro identifies a flaky invocation-order assertion while confirming one winner, one conflict and unauthorized mutation denied. No fix was made; the unexpected Vitest worker exit remains unverified. Neither is a blanket helper-prerequisite failure.

## Full migration continuation

The user rejected the standalone terminal as a Shellfox replacement and explicitly requires the full migration. Stage numbers describe acceptance, not a serial work queue. Real, tested core/host and application work starts alongside the remaining stage 1 work. No gate has passed merely because this sequencing changed. Do not stop after another terminal demo, or add empty host/client/mobile crates to imply progress.

### Why the prototype was not usable Shellfox

The inspected starting implementation had these concrete gaps. Concurrent fixes need their own evidence.

- `options.rs` defaulted to fixture mode, which creates no PTY. `Root` rendered one `TerminalView`, not sessions, machines, tabs, files or settings. The smoke route exercised cmd/PTY/Ghostty without initializing GPUI.
- `PtyConfig::builder` cleared the environment, set PATH to System32 and redirected HOME/APPDATA/TEMP to scratch. The default shell was cmd, launched in scratch. `ls` is not a cmd builtin; installed `pi`, PowerShell discovery and user tools were excluded by construction. This is a shell/profile/environment integration failure, not evidence that Ghostty cannot run those programs.
- The initial view had no mouse/selection/link wiring. Native input composition, shaping, accessibility and representative TUI behavior were still unaccepted.
- Run-specific directories were right for disposable smoke tests, but cannot provide persistent session/settings behavior across application launches.

Fix profiles, inherited tool discovery, environment precedence and app navigation. Do not invent an `ls` alias, hardcode a user's executable path, weaken process ownership, or silently read/write the user's production profile/data. Keep fixture/smoke isolation explicit and separate from interactive shell policy. Shell commands may modify files and startup profiles may have side effects; an isolated app database is not a shell sandbox.

### Current parallel ownership and integration contracts

These are agreed work boundaries, not completed implementations. Only the plan/inventory worker edits these two planning documents. Heather coordinates root Cargo/lockfile edits.

| Owner | Owned implementation | Integration boundary and first required evidence |
| --- | --- | --- |
| Andre | `crates/shellfox-core/`, `crates/shellfox-host/`; validated contracts, session metadata, prefs, SQLite | Real reopen/rollback/validation/idempotency tests; API shared directly with Eva. Initial create is metadata, not implicit PTY spawn. Cleanup prepare/commit contract follows before deleting live terminals. |
| Gavrila | `crates/shellfox-terminal/`; shell profiles, inherited environment, owned PTY lifecycle | Portable profile preference and env contract with core/host; `PtyConfig` stays a launch adapter. Preserve trusted supervisor/Job gates and explicit unavailability on unverified platforms. |
| Eva | `crates/shellfox-desktop/`; integrated Shellfox workspace, fields, sessions/tabs/settings/files presentation | Default application enters workspace; fixture explicit. Consume Andre's metadata API, Eirik's runtime and Electra's files API; no parallel SQL/session store, PTY owner or parser. Coordinate terminal `app.rs`/renderer edits with Heather. |
| Eirik | `crates/shellfox-runtime/`; single PTY/parser owner, bounded commands/events, epochs/replay | Integrate Gavrila's owned PTY, Andre's launch/cleanup contract and Heather's Ghostty APIs. Runtime alone replies to terminal queries; views must not also parse raw output and reply. |
| Electra | `crates/shellfox-files/`; actual scoped file API | Eva consumes typed directory/read/write behavior. Resolve scope from host session/tab context; preserve canonical paths, revision conflicts and bounded I/O. Do not trust a view-supplied cwd as authorization. |
| Heather | Public Ghostty wrapper/sys interactions, formatter/continuation/mode bounds; root Cargo coordination | No internal ABI. Coordinate limited recovery with Maurice; authoritative 15-member root lock is frozen. Future member/dependency additions require a separate coordinated refresh. |
| Maurice | Bounded formatted active-buffer recovery and tests | Reports 12/12 real Ghostty+xterm tests passed and 3 FullState counterexamples. Limited `FormattedActiveBufferV1` is not saved-cursor/inactive-buffer/Kitty-mode restoration. Retain Node ANSI. |
| Musa | Read-only runtime observer hub | `observers.rs` shares Delivery's journal with bounded observer leases/credit. Written, not wired or parity-accepted; preserve permissions/exclusive-owner rules and no observer input authority. |
| Mulavarman | `crates/shellfox-transport/`; Go helper/proxy adapter and native transport boundary | Actual bounded adapter tests; peer-ID/endpoint allowlist and authenticated native routing. No client state machine, SQLite or GPUI. |
| Russell | `crates/shellfox-client/`; typed operations, connection/approval state and injected transport | Coordinate server request/reply/event version early. Consume runtime epochs/replay/ACK contracts; no local host/process authority in mobile. |
| Walter | Rust headless service and CLI, using Russell/Mulavarman's agreed boundary | Single host/runtime registry, bounded authenticated local service and typed CLI. Root Cargo changes through Heather; no SQLite bypass in CLI or GPUI linkage. |
| Euphemia | Existing `shellfox-service::Service` remote auth/approval backend and full RN wire adapter | Reuse the one host/runtime registry, protocol/client/transport. Real RN-compatible `/v1` behavior, not a permanent experiment-only protocol; no second Service. Integration tests still pending. |
| Guarocuya | New Unix terminal adapter modules only | Coordinate integration points with Gavrila without editing his Windows/profile/environment files. Native Linux/macOS ownership and cleanup acceptance before un-gating. |
| Madelyn | SSH private preferences and known-host behavior | Host-only secrets and typed public DTOs; wire to real SSH/runtime later. No accept-all host keys or credential exposure. |
| Salahuddin | Rust package/resource integration | Actual build/resource/packaged-smoke paths; no release, publication or signing bypass. Coordinate Cargo with Heather. |
| Carter | Actual terminal Rust bridge/header and RN specs/typed adapter | Outbound-only mobile APIs; codegen and 5 typed adapter tests passed, not native compilation. Coordinate the native terminal ABI with Shakuntala/Hafsa. |
| Lakshmana | Rust mobile client companion C ABI/runtime/native transport completion | Uses Russell's actual client/cursor and shared cancellation types. Bind delivery to native terminal consumption; no implicit ACK or host/process authority. |
| Shakuntala | Android native terminal/client integration | JNI/Kotlin renderer/input/coordinator work is present; native compile, consumed RN wiring and Android device acceptance still pending. |
| Hafsa | iOS native terminal/client integration | Objective-C++ owner/frame/keyboard/text-input work is present; native compile, consumed RN wiring and iOS device acceptance still pending. |
| Juan | Go/C# prerequisite provisioning and security-proof tests | Reports project-scoped Go 1.27.3 and active helper builds passed, `test:tailscale` passed, focused host/automation/RPC/endpoint 36 passed/2 skipped, native 79 .NET plus all 5 pipe-run passed. See the attributed worklog; no full unit rerun. |

Minimal crates remain focused. `shellfox-core` owns portable DTOs, preferences, errors and validation, without GPUI, PTYs, filesystem access or helper processes. `shellfox-host` owns storage and session metadata/cleanup preparation. `shellfox-runtime` owns live processes, terminal state and control/delivery, using `shellfox-terminal` for PTYs. `shellfox-files` owns scoped file I/O; host integration supplies trusted terminal context. GPUI only presents state and submits commands. `shellfox-client` now implements outbound behavior against `shellfox-protocol` and an injected transport, never host or desktop. `shellfox-cli` uses host/client without GPUI. Actual `shellfox-mobile` terminal/client bridges and Android/iOS consumers are under construction; require native compilation and consumed app paths before claiming integration.

Existing host `{storage,sessions,tabs,lifecycle,boundary}.rs`, runtime `{api,delivery}.rs`, files `{contracts,paths}.rs` and desktop `{workspace,settings,dialogs}.rs` are now being written. Extend these responsibilities rather than creating competing stores, services or views. Move the prototype's PTY ownership out of the view through the real runtime integration; do not create two PTYs for a tab or two authoritative query responders.

Observed during this continuation, before validation: core now has typed UUID IDs, `Session/Tab/Snapshot`, `Preferences/PreferencesPatch`, `TerminalLaunch/TerminalIdentity/PreparedClose` and `ChangedEvent`. Host files expose `Host::open`, `snapshot`, `create_session`, tab/session edits, `update_preferences`, `changed_since`, env setters, `terminal_launch` and `import_legacy`; storage defines the agreed filename, application identity, schema v1, transactions, WAL and foreign keys. Legacy import fixtures/tests are present; scoped results are recorded below. Andre now reports final locked core/host 30 passes and Clippy with warnings denied after the root refresh; see the worklog for the 5/3/2/12/8 breakdown and limits. Runtime and files crates also now have implementation files, but their integration and acceptance are not established by that fact. Host now defines `register_terminal/terminal_stopped`, `prepare_close_tab/prepare_delete_session`, `commit_close/cancel_close`; these are trusted in-process adapter operations, not remote cleanup authority. `Pending/Archived` are metadata states, not process evidence. These smaller local `Snapshot/Preferences` types are not yet `ManagerSnapshot/SettingsDto` wire replacements, and `changed_since` is not yet the bounded client event stream.

#### Implementation observed, not accepted

This table records concrete source inspected while workers are editing. Attributed and independently executed checks are distinguished in [the worklog](ghostty-rust-worklog.md). Latest authoritative root lock is READY/frozen for all 15 members, including SSH/service/desktop edges; this supersedes pending-refresh wording. Future remote member 16 is not added or covered. Members are not OS targets.

| Component | Actual source/API now present | Still not established |
| --- | --- | --- |
| Core/host | UUID types, metadata requests/store/cleanup; Andre reports locked 30 tests passed, 5 core/3 boundary/2 cleanup/12 legacy/8 metadata, plus Clippy `-D warnings` | Full SettingsDto/ManagerSnapshot parity, runtime/UI cleanup integration and complete legacy settings/env migration remain open. Smaller metadata snapshots are not full host wire compatibility. |
| Runtime | Bounded commands/events/input and one PTY/parser owner; Eirik's latest 16 unit plus 6 real owned-PTY integration tests and Clippy passed, superseding the earlier 13-unit count | App/native UI ownership integration, complete query/title/cwd/recovery and platform acceptance remain open. Explicit recovery capabilities now include `VisibleScreen` and limited `FormattedActiveBufferV1`, never FullState. Musa's observer hub is written but unwired. Exact runtime test commands are not supplied in this handoff. |
| Files | Trusted scoped list/stat/text/revision-write/preview/mutations; Electra reports Windows 3 race plus 11 integration passes and Clippy; Darwin arm64/Windows arm64 all-target checks passed | Cross-compilation is not native acceptance. Full session/tab wire mapping, chunk transfers, HTML assets, WSL scopes and app/device behavior remain open; local file DTOs are not the full RN wire adapter. |
| Protocol | Validated DTOs/sealed commands/shared cancellation; strict `Reply` discriminants/null branches and null-versus-omission preservation; Russell reports 11 wire tests and TS wire fixtures passed | Full live-server/device and recovery-format compatibility remain open. Passing selected fixtures is not full-state recovery or complete method coverage. |
| Client | Actual pair/execute/poll/reconnect/suspend/disconnect, RAII cancellation/admission release and epoch/sequence `ViewCursor`; Russell reports 9 client plus 6 cursor tests passed, 26 with the 11 wire tests | Real server/RN transport and app integration remain separate. Polling never ACKs; `consumed()` constructs a typed ACK without sending it. No full recovery claim. |
| Transport | `config.rs`, `protocol.rs`, `lib.rs`; helper probe/status/peer/frame decoding with bounded commands, redacted errors and explicit protocol validation | Helper process/HTTP adapter execution, agreed trait implementation and real signed-host/device integration. The desktop helper's command channel is not the mobile Go request channel. |
| Desktop | Workspace/settings/dialogs/terminal modules and sole-runtime adapter written; latest 18 UI unit tests and Clippy passed | Final build awaits exhaustive observer-event handling. No launched-app/native ownership acceptance; host/files/full navigation and platform behavior remain open. |
| Headless/CLI | Walter assigned the actual service/CLI against Russell/Mulavarman's common boundary | Runnable server/CLI and authenticated ownership/control/recovery tests. No headless acceptance is inferred from desktop's smoke route. |
| Mobile | Terminal/client ABI and native completion bridge; Carter reports terminal bridge 8 real-Ghostty Windows passes plus mobile tsc; client ABI compiles but 11/12 tests pass, one cancellation expectation fix underway; codegen and typed adapter 5 pass | Android/iOS native compile/link, client generation/reset handshake, real transport, screen adoption and device acceptance remain open. Do not mark the client ABI suite all-pass. Windows bridge evidence and TypeScript checks are not native mobile acceptance. Accepted bytes are not consumption ACKs. |

For the later compatibility adapter, shared methods keep the existing names and semantics below. Rust can use snake_case internally and camelCase at serialization boundaries. Eva uses Andre's current typed local API directly; do not make her wait for every RPC method or pretend the adapter already exists.

| First contract | Preservation requirement |
| --- | --- |
| `getSnapshot`, `subscribe` | Monotonic revision and `ChangedEvent` reason; publish only after committed mutations. Snapshot is metadata, not proof a shell is running. |
| `createSession({cwd,title?,requestId})`, `addTab` | UUID request idempotency, no duplicate records or automatic replay of uncertain launch. Initial metadata creation and terminal launch are separate. UI checks returned tab/lifecycle state rather than assuming creation spawned a terminal. |
| `renameSession`, `renameTab`, `reorderTabs`, `setSessionPinned` | Full saved tab permutation, stable IDs/order, user title versus process title, persisted `pinnedAt`; pins prevent automatic deletion. |
| `saveSettings`, `setSessionEnv`, `setTerminalEnv` | Omitted `terminalEnv` preserves the host's latest value. Portable profile IDs, no scratch-root coupling. Host OS controls env-name case policy; inherited < global < session, managed host identity last. |
| `closeTab`, `deleteSession`, later settle/retry/activate | Prepare validates membership and captures generation; owned cleanup runs without an open SQLite transaction; commit revalidates identity before deleting. Failure retains ownership/metadata and reports an error. Prepare/commit APIs now exist in host source; runtime/UI integration and tests still need evidence. Settlement does not close a shell. |
| `attachTerminal`, `takeTerminalControl`, `writeTerminal`, `resizeTerminal`, `acknowledgeTerminal`, `detachTerminal` | Preserve generation, control epoch, sequence and consuming-client ACK. `userInitiated=false` marks parser replies; `settledAt` prevents stale input after settlement. Metadata APIs alone do not implement these. |

### Storage decision

Use **`shellfox-rust-v1.sqlite3`** under an explicit stable experiment data directory. This filename is agreed with the host/UI workers. Application identity and version checks must reject foreign and newer databases before mutation. Keep schema migrations transactional, foreign keys enabled, bounded busy handling and atomic settings/session updates. Disposable smoke/test directories remain separate. Never auto-discover, open, migrate or replace production `manager.sqlite3`.

The existing `Repository` uses `user_version=5`, WAL, foreign keys and a 5000 ms busy timeout. Its tables are `sessions`, `tabs`, `operations`, `settings`, `preferences`, `terminal_metadata`; opening it runs migrations and bundled-rule upgrades. A new store with its own schema is not v5 compatibility. Test v1 reopen, corrupt/foreign/newer refusal, transaction rollback, request-id reuse, tab-order uniqueness, cascading metadata cleanup and concurrent settings/env updates. Stored data needs runtime validation too.

Complete explicit import into a fresh Rust store from a consistent copied legacy database, using SQLite backup or a quiesced copy including WAL state. Test representative v1-v5 fixtures and refusal of newer versions. The in-progress `legacy.rs` currently imports sessions and appearance/default-profile preferences only; it deliberately excludes tabs, operations, env, registrations and credentials. Baseline `service.ts::initialize` also deletes saved tab records without adopting their processes. That is not permission to lose session/global env or other user preferences without a report and a tested migration decision. Full metadata/settings import remains open. Preserve IDs, titles, cwd, pins and supported settings; explicitly report omitted metadata. Retain user/process-title and tab-order behavior for the new store. Never adopt PID/start-time/window/Job registrations, restore live ownership or restart commands from records. Copy/import SSH and network state only through separate reviewed opt-in paths, not a generic database import. Do not persist terminal output, credentials or process command lines in the new metadata store.

### Public ABI recovery blocker

`vt-1.0.1` public formatting exports the active screen, not inactive buffers or saved cursors. Pending-wrap reconstruction can clobber a saved cursor; runtime tests now encode saved-cursor and alternate-to-primary divergence. Heather's public formatter/continuation/mode bounds and Maurice's `FormattedActiveBufferV1` cannot turn absent state into `FullState`. Actual capability enum now names `VisibleScreen` and `FormattedActiveBufferV1`; Maurice reports 12/12 Ghostty+xterm passes plus 3 FullState counterexamples. Limited fidelity remains explicit; full app/wire recovery is not accepted.

Keep the existing Node ANSI-compatible recovery path. Closing full-state parity requires an adequate upstream public API or a new explicitly negotiated/versioned Ghostty snapshot format with tested legacy fallback. Do not use internal ABI, unbounded output retention/replay or persisted output as a substitute. Unsupported full-state requests must fail explicitly, never receive silently downgraded ANSI. Other passing crates do not close this migration blocker. See the worklog for source evidence and required tests.

### Reusable protocols and client behavior

Reuse these contracts and their tests rather than inventing a second incompatible Rust protocol.

- `src/shared/schemas.ts` is the validation reference, not only TypeScript interfaces. Preserve UUIDs, UTC timestamps, limits, strict object validation, optional/null distinctions and `Result<AppError>`. Add JSON fixtures consumed by both Zod and Rust for each migrated method. Keep unimplemented methods out of advertised capabilities; a smaller Rust DTO is not wire parity.
- `host-schemas.ts` defines loopback endpoint/envelope v1, `host.open/close/poll`, manager/automation methods and event envelopes. Loopback requests are bounded at 1 MiB, responses at 16 MiB. Secure endpoint publication and same-user host ownership are separate from session storage.
- `control.ts`, `delivery.ts`, `terminal-client.ts` and `mobile/src/client/README.md` define exclusive-v1 ownership. Creator reserves a terminal; only the owner gets contents; blocked views clear contents. Subscribe before attach, reconcile pre-response events, ACK only after consumption, enforce 128 KiB live credit and bounded replay, and reattach after reconnect/takeover. Disconnect never kills shells; no mutation or input auto-retry.
- `screen-mirror.ts` and `terminal-compat.ts` supply ANSI screen restore for existing xterm and ordinary CLI terminals. Ghostty binary snapshots cannot replace this unannounced. Port authoritative query replies, title/OSC 7 cwd, alternate screen/modes and parser continuation; replay must not repeat replies or clipboard effects.
- `mobile/src/client/{client,protocol,operations}.ts` already implements approval/rejection, suspension, connection epochs, stale-reply invalidation, at most eight operations plus an event poll, coalesced visible interest and no input replay. Translate this into shared Rust client state with transport/clock injection, then bind RN; do not put host APIs into a remote-only mobile library.
- `file-contracts.ts`, `file-transfer-schemas.ts`, `terminal-upload-schemas.ts` and `main/files/` define scoped paths, revision conflicts, bounded chunks, actor cancellation, staged writes and revocation checks. Preserve separate local desktop dialogs and remote transfer permissions. A local directory-list widget is not the files API replacement.
- CLI help, `automation-schemas.ts`, `ssh-schemas.ts` and their tests cover behavior beyond sessions. Automation uses ordinary stdout/stderr pipes with backpressure and read-only terminal views; do not force it through an interactive PTY. SSH passwords/private options stay host-only, and omitted versus null password means preserve versus clear.

### Next delegation: Go transport reuse

A focused `shellfox-transport` crate can start after core event/error types and the injected client transport contract settle. It is a real adapter with parser/process/HTTP tests, not a Tailscale rewrite. Keep it independent of GPUI and SQLite. A separate `shellfox-client` owns typed operations, approval/connection epochs and polling, with an injected peer-ID/allowlisted-endpoint transport. It must use runtime terminal identities/epochs/ACK semantics, not create a second parser or auto-ACK before view consumption. Preserve the remote method whitelist separately from the local Manager API; desktop settings/dialogs and host-stop authority must not leak into mobile.

1. Port `src/main/remote/helper.ts` against `native/tailscale/{protocol,helper}.go`. The helper has `--probe` protocolVersion 1 and NDJSON commands `start/status/peers/login/logout/stop`, correlated replies and unsolicited status events. Start payload is `stateDirectory/hostname/authKey?/backendUrl/backendSecret/proxySecret`; retain separate secrets and private state paths. Go reads command lines up to 64 KiB. Bound Rust decoding/pending requests, reject malformed/mismatched replies, enforce 30-second deadlines and owned-child shutdown, and never forward raw stderr or credentials to UI/logs.
2. Reuse the Go loopback `/proxy` route with its bearer proxy secret, peer-ID routing and `/v1/discover|pair|request|events|disconnect` allowlist. Do not use an arbitrary HTTP client that bypasses tsnet, follows redirects or replays POST mutations. Bound JSON requests at 1 MiB and remote responses at 8 MiB; separate long polls from ordinary operation admission.
3. Incoming Go requests carry HMAC-signed peer/timestamp headers after WhoIs and same-user/untagged/current-map checks. Port `main/remote/auth.ts` verification byte-for-byte, including body digest, method/path binding, constant-time compare and 30-second skew bound. Then implement approvals/view permissions in Rust host; a signed identity alone is not approval. Keep backend and proxy secrets off mobile.
4. Reuse `native/mobile` through its existing gomobile-supported string/error API. `Start/Status/Peers/Hosts/Login/Logout/Request/NetworkChanged/Stop` stay outbound-only. Inject `Request(peerId,path,body)` into the Rust client through native Android/iOS glue, not the desktop helper subprocess. Preserve request/poll admission, response limits, route-ownership revalidation and connection-pool cancellation.
5. Prerequisites for real acceptance are isolated state fixtures, the pinned Go 1.27.3 toolchain/helper build, private-permissions and ownership helpers, a Rust host backend implementing signed `/v1/*`, paired test peers and device SDKs. Until that backend exists, test helper decoding, signing vectors and fake transports offline; do not claim an end-to-end connection. Existing Go tests cover bridge/auth/ownership/nethealth and mobile routing/request/pool behavior. Real Android/iOS network changes/backgrounding still require devices. Never reuse the installed node's credentials.

### Next delegation: real headless server and CLI

Walter now owns this work and coordinates the common protocol/transport boundary with Russell and Mulavarman. A server owns one `Host` and one runtime map per tab; CLI consumes the typed client. Neither depends on GPUI. Actual `shellfox-service` owns one Host/runtime registry and `shellfox-cli` consumes the shared client. Euphemia extends that same Service for compatible remote RN behavior; no second Service or runtime owner.

- Start the trusted supervisor entry before parsing arguments or initializing services. Explicit isolated data selection and same-user single-host ownership precede opening SQLite or PTYs. Secure endpoint publication uses an experiment namespace, not the installed host's endpoint. Reuse ownership/private-permissions semantics in `src/host/{ownership,rpc,rpc-clients}.ts` and `src/main/control-endpoint.ts`; do not bypass unavailable helpers.
- Metadata requests call Andre's actual typed `HostRequest/Host::request`. Launch resolves `terminal_launch`, registers a fresh generation, then starts Eirik's runtime. Keep failed-start cleanup owners. Runtime handles output, replies, leases, ACK/replay and bounded events; server maps them to each authenticated view. Close/delete uses host prepare, confirmed runtime cleanup, then commit, never view-reported success.
- Implement local view open/close/poll, request-ID correlation, bounded bodies/responses/polls/operation concurrency and shutdown reply draining. View disconnect detaches without killing the shell. CLI raw attachment must not duplicate host parser replies or claim a full-screen restore from visible-cell ANSI alone.
- Same method spelling is not wire compatibility. Current core create requires a profile preference and returns its smaller snapshot; baseline create/session/settings replies and `host-schemas.ts` envelopes differ. Euphemia is implementing the production-compatible RN adapter on the existing Service. Preserve local experiment-data isolation without making the remote protocol permanently experiment-only. Require signed `/v1` routes and existing Zod request/result/event acceptance, and reject unsupported recovery fidelity explicitly.
- First runnable CLI increment is status, session list/create/rename/pin, prefs/path and real terminal create/close/attach against the running Rust server, with JSON/errors and no direct database access. Follow with settle/env/order/stats, scripts, network/approvals, automation, SSH, service/update and completion parity. No metadata-only command should secretly spawn a PTY.
- Required tests include hostile/malformed/oversized loopback requests, wrong secret/origin/view/epoch, duplicate host ownership, endpoint replacement races, stale create replies, blocked-view input, poll saturation, slow consumer, disconnect versus host-stop cleanup, stop-response delivery and no-display launch. Use isolated owned processes; production sessions and node state stay untouched.

### Next parallel scopes beyond Windows desktop

Do not wait for another Windows UI signoff before starting these real integrations. Each scope must land code consumed by the app or a runnable native/package test, not an unused abstraction. The protocol/client/server workers retain their current files; Heather coordinates Cargo changes.

| Priority and scope | Existing source to translate or retain | Owned change and acceptance dependency |
| --- | --- | --- |
| 1. Unix owned lifecycle | `main/terminal/{unix-close,pidfd-preflight,darwin-supervisor,linux-scope}.ts`, `main/terminal/native/darwin-terminal-supervisor.c` and its model tests | Work inside terminal OS adapters, coordinate config/profile edits with Gavrila. Keep the active Darwin C helper behind its validated control protocol initially. Prove startup/birth identity, supervisor readiness, descendant cleanup, failed-cleanup retention, timeout and parent-loss behavior. Preserve Linux pidfd guarantees and optional systemd/OOM scope policy. Native Linux/macOS runs precede un-gating; cross-compile or a process-group kill is not acceptance. |
| 1. Android/iOS client bindings | `mobile/src/client/`, `mobile/src/network.ts`, Android `ShellfoxNetworkModule.kt`, iOS `ShellfoxNetwork.mm`, `native/mobile/` | After Russell's typed API settles, add a real `shellfox-mobile` client binding and consume it through an RN adapter on both OSes. Inject the existing Go request API; preserve no-backup/protected network state, immediate request admission, separate polls, deadlines and sanitized errors. Test connect/pair/reject/cancel/suspend/resume/stale completion on both native bindings. No dependency on host/runtime/local PTYs. Do not import original checkout changes. |
| 2. Native mobile terminal | `mobile/src/terminal/{TerminalView,native-text-input,page-keyboard}.*`, `useStickyModifiers.ts`, existing native IME scripts and Heather's Ghostty APIs | Build actual Android and iOS native view bindings consumed by `TerminalView`, with native parser/render/input and bounded output/frame/ACK queues. Keep GPUI out. Validate RN 0.88 native component registration, API 24 and effective iOS minimum with real SDKs. Hardware/software IME, delete/composition, touch scroll/selection, clipboard/link policy, safe-area/keyboard resize, accessibility and suspend/recovery are required. Retain the explicit WebView transition until both are accepted. |
| 2. Windows elevation and WSL | `main/terminal/windows-elevated.ts`, `native/Shellfox.ElevatedTerminal/`, guest-close/pidfd/profile/WSL tests | Retain authenticated C# helper initially, port bounded framing/readiness/ACK/confirmed cleanup into terminal adapters. Elevation is explicit per tab, never default/headless/mobile. Job membership does not prove WSL guest ownership; port guest preflight/identity and nested marker behavior before enabling. Coordinate Windows adapter edits with Gavrila. |
| 2. SSH domain and connections | `shared/ssh-schemas.ts`, `main/ssh/{storage,keys,known-hosts,putty,client,pty}.ts` | Independent typed host-only store/key/known-host behavior can start now, then wire actual SSH PTYs through the runtime adapter. Preserve password omitted/null/replace, private options excluded from DTOs, case-insensitive names, atomic private writes, key conversion and host-key verification. Test fixtures plus real isolated SSH peer later; no password argv/logs, insecure accept-all, or metadata-only connection button. |
| 3. Packaging and updates | `scripts/{headless-package,headless-smoke,terminal-native-helpers,mobile-native,package-remote-smoke}.mjs`, `main/update/`, `mobile/src/updates/`, existing workflows | Build real Rust desktop/headless artifacts and manifests/resource adapters; retain Go, Darwin and elevated helper paths/notices. Prove isolated installed-resource lookup, ABI/DLL loading, no-display headless launch, CLI/service paths, rollback/shutdown and verified update selection. Keep desktop/headless/mobile update authority separate. Native x64/arm64 jobs, signing/device prerequisites and license checks stay explicit; no release/publish or installed-app replacement is authorized. |

These scopes are now assigned: Guarocuya's new Unix modules, Madelyn's SSH private preferences/known-hosts, Salahuddin's packaging/resources, Carter's C ABI/spec followed by Android/iOS consumers, and Juan's helper provisioning/security tests. The remaining mobile native graphics/input work still requires actual Android and iOS toolchains and consumed bindings. No platform support has been accepted by this planning pass. Porting retired `Shellfox.Native`/`Shellfox.Linux` window backends would waste work and weaken the selected architecture.

### Integrated increments and full remaining checklist

All boxes below are open for Rust application parity. Worker code landing is not acceptance. For every row record changed files, exact test command/result, native platform/client evidence and blockers. Windows x64 component evidence above is retained, not broadened to other targets.

| Increment | Runnable result and remaining acceptance |
| --- | --- |
| A, host and workspace now | [ ] Stable isolated store, session list/create/rename/pin/delete, tabs/add/rename/order, settings/env survive relaunch. Metadata and live terminal states remain distinct. Core/host tests run without GPUI. |
| B, usable shell now | [ ] Default workspace opens chosen folder/profile, inherited tools/PATH available; PowerShell `ls` and installed `pi` work where present. Missing program/profile produces a real error. Fixture remains explicit. Test process containment, launch failures and no cleanup regressions. |
| C, host-owned terminals | [ ] Transfer PTY map/query authority from view to host; prepare/commit close/delete, generation rejection, failed cleanup retention and final-output draining. Selecting/hiding a tab does not recreate or kill its PTY. Confirm quit/update with live work. |
| D, complete terminal | [ ] Mouse tracking versus selection, drag/copy/paste, scrollback, OSC 8 and file links, focus, shortcuts and terminal Ctrl+C. Native IME/AltGr/dead-key/Option, emoji/wide glyphs, font fallback/shaping, DPI, accessibility, representative TUI and backlog/resize/load acceptance. |
| E, shared client/control | [ ] Integrated client/control, bounded events/ACK/replay and stale epochs. Hidden/blocked views do not write/resize. Full-state recovery is blocked by public ABI gaps; explicitly negotiate the limited formatted projection and retain legacy Node ANSI until upstream API or a new snapshot protocol plus legacy fallback is proved. Test RN/xterm/CLI without replay side effects. |
| F, files host and desktop | [ ] Cwd-aware directory navigation/create, WSL/UNC scopes, text/image/binary reads, text editing and revision conflicts, Markdown/HTML/SVG/image previews, image editing, dirty drafts/confirmations and file-line links. Chunk uploads/downloads, cancellation, symlink/path confinement and revocation/staging races pass. Decide a safe HTML preview renderer with tests, not an omitted preview or privileged arbitrary webview. |
| G, host RPC and CLI | [ ] Authenticated endpoint discovery, same-user single host, status/start/stop/service ownership, keep-awake, all session/terminal/settings/path/network/approval commands, named scripts, completions, JSON/errors/confirmations, current-marker context and raw terminal attach/detach/takeover. No display/GPUI linkage in headless artifacts. |
| H, remote transport and host | [ ] Go helper adapter, signed incoming routes, discovery/cache/login, pair/approve/reject/revoke/forget, remembered peers, per-view permissions, uploads, bounded polling/slow clients and health recovery. Existing RN client talks to Rust host before replacing its client logic. |
| I, activity and session policy | [ ] Agent rules/process identity tracking, fresh stats/processes, busy echo suppression, visibility interest and reconnect activity. Pin ordering/pagination, settle/unsettle confirmation, stale settled-input rejection, retention/delete policy and user/process titles match baseline. |
| J, SSH and automation | [ ] SSH profile CRUD/import, host-only credentials, key conversion/known-host verification/PTY/reconnect; ordinary-pipe automation start/read/write/eof/stop/show, exit codes, bounded replay and descendant cleanup. No secret argv/logs or forced PTY conversion. |
| K, complete desktop | [ ] Pixel-perfect committed Electron across every state: chrome/controls, machines/sidebar/sessions/tabs, menus/dialogs, files/previews/editor, all settings/status/feedback. Controlled actual Electron goldens plus paired GPUI pixel/geometry/fonts/colors/icons comparisons; independent full interaction/accessibility matrix and native OS evidence. No approximations, masking required content, dummy controls or reduced UX. User approval for every deviation. |
| L, mobile shared client | [ ] Typed native Rust client bindings consumed by existing RN screens on both OSes; retain Go transport, remote-only permissions and existing local preferences. Prove suspension/resume/cancel/rejection/stale replies and don't overwrite original checkout changes. |
| M, mobile native terminal | [ ] Android and iOS Ghostty native component, bounded native render/data/consumption-ACK path, queue/parser/display reset and stale-generation fencing on control loss/reconnect, touch scroll/selection, keyboard/IME/deletion/sticky modifiers, clipboard/link policy, safe areas, keyboard resize, Android back and accessibility. Remove WebView/xterm only after both native clients and negotiated recovery pass. |
| N, platform closure | [ ] Unix identity/supervisor implementation and native Linux/macOS cleanup before enabling PTYs; Windows normal/elevated and WSL cleanup, x64/arm64; Android API 24 and effective iOS minimum supported. No numeric-PID fallback, inherited production host identity or silent platform drop. |
| O, storage and distribution | [ ] Copied legacy v1-v5 import and rollback, separate SSH/network migration, release artifacts/resources/notices/signing/CLI launchers/service paths, desktop/headless/mobile updates and full OS/arch matrix. No installed-app replacement or release without authorization. |
| P, final verification and removal | [ ] Relevant TypeScript/native/Rust/platform/packaging checks, baseline race/worker failures resolved, native GUI/TUI/device evidence and comparable release CPU/memory/startup/input/output/scroll/reconnect measurements. Remove old runtime/dependencies only after all reachable behavior is replaced; no empty adapters left behind. |

A and B run in parallel with D and platform/build work. C follows the agreed host/PTY lifecycle contract; Eirik is implementing runtime and Electra is implementing F's scoped API alongside the workspace now. Once core/host contracts stabilize, client, CLI and transport groundwork can run independently with typed boundaries and contract tests; H integrates those pieces. L can start transport/client bindings before M's graphics work. Neither this ordering nor a Windows screenshot closes stages 1-5. Keep production Electron/Node/mobile available until their replacements meet the relevant rows.

### Continuation evidence received

[The continuation worklog](ghostty-rust-worklog.md) records Juan's resolved helper prerequisites, the TypeScript queue-order assertion, actual protocol/client/mobile source progress, reported codegen and 5 independently passing typed adapter tests. The worklog also records Salahuddin's later full unit pass, 338 files/4039 tests/59 skips, packaging 8 passes and Electra's files results. These do not close Rust app/native integration or recovery/platform gates; preserve the earlier assertion-flake history.

The integration lead reports Gavrila's four shell-profile/usability tests and 11 lifecycle tests passed. Source inspection identifies `tests/shell_profiles.rs` and `tests/lifecycle.rs` under `shellfox-terminal`, covering PowerShell-default/availability, interactive `ls`, inherited executable resolution and isolated history/state, protected environment checks, explicit cmd and lifecycle containment. Both pwsh and Windows PowerShell usability were exercised. `node`/`pi` command resolution was tested without executing the user's account-bound Pi application. This is not Pi login/authenticated-session or full TUI acceptance. Exact execution commands were not supplied in this handoff; these are attributed worker results, not an independent plan-worker rerun. Keep the broader A-P checklist open.

Continuation plan-worker verification: `pnpm typecheck` passed in 10.2 seconds with a 150-second outer timeout, including mobile typechecking/generated terminal HTML. `git diff --check` passed. No new Cargo/GUI/host/files/runtime acceptance was executed by this documentation worker, and the documentation worker did not rerun the full suite. Salahuddin's later pass is attributed separately; no duplicate rerun is needed. No implementation, instructions, original checkout or production data was changed by this worker.
