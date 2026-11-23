# Shellfox Rust, GPUI and Ghostty migration

Active delivery priorities, work ownership, stopping conditions and deferred research are in [SHIP_DECISIONS.md](docs/SHIP_DECISIONS.md). Consult that file before resuming any older task below. Windows, Linux and Android are equal P0 targets. Historical prototype/contract next steps are parked unless the active delivery path explicitly reopens them.

## Read this first

This is the implementation handoff for the `experiment/ghostty-rust` branch.

- Worktree: `P:/workshop/Windows_Worshop/oira666/shellfox-ghostti`
- Baseline: `c8cafb8`, committed `main` when this worktree was created.
- Original checkout: `P:/workshop/Windows_Worshop/oira666/shellfox`
- Requested Shellfox session title: `shellfox-ghostti`.
- Ghostty is the upstream project's spelling. `ghostti` is intentional in the requested folder/session name.
- No application implementation was performed during initial planning. Implementation progress is recorded separately below.

The original checkout has uncommitted mobile UI, preference, gesture, file-browser, and terminal-input changes. Those changes were deliberately NOT copied, committed, stashed, or reverted. This worktree starts from committed HEAD. Do not assume its mobile code contains that ongoing work. Coordinate before importing those changes; do not overwrite or silently replace them from this branch.

Read this worktree's existing `AGENTS.md` and applicable global instructions before coding. Do not create or modify any `AGENTS.md` without explicit user permission. Keep temporary artifacts under this worktree's root `tmp/` and clean up disposable artifacts. Every shell command requires a timeout.

## User-approved direction

The user requires the complete Shellfox migration to Ghostty terminal emulation and Rust application logic. The standalone terminal prototype was rejected as a replacement for Shellfox. This remains an isolated, unreleased worktree, but the delivery target is the full application, not another terminal demo. The agreed architecture is:

- Desktop UI: GPUI and Rust, replacing Electron/React after validation.
- Mobile UI: retain React Native and TypeScript on Android and iOS.
- Shared application/client logic: Rust, exposed through narrow typed mobile bindings.
- Host and CLI: Rust, with the headless host independent of graphical frameworks.
- Terminal engine: `libghostty-vt`, accessed through its public C ABI.
- Long-term mobile terminal: a native React Native component using Ghostty, not a permanent xterm WebView.
- Retain embedded Tailscale in Go initially, and necessary OS-specific integration code.

Implement integrated increments with runnable results and tests. Core/host/session storage and GPUI application work proceed in parallel with remaining terminal work; stage 1 acceptance is not a prerequisite for starting them. Do not replace every subsystem without validation or stop after the terminal works. Do not publish, tag, trigger releases, replace the installed Shellfox, or run experiments against production data without separate authorization.

GPUI is the selected framework to evaluate, not permission to ship missing platform functionality. Report blockers with evidence. If a required platform cannot be supported without a major architectural change, ask the user before changing the agreed direction or dropping support.

## Exact Electron UI and functionality acceptance

User requirement: "interface should be pixel-perfect exactly the same as electron app now. And functionality." GPUI must match the committed worktree's current Electron desktop in every UI state with full functionality, not approximate styling, simplified controls or reduced UX. Any deviation needs explicit user approval. Do not import dirty mobile work without coordination.

[Visual parity requirements](docs/electron-gpui-visual-parity.md) define isolated actual Electron goldens, identical OS/DPI/fonts/theme/window/data state, paired GPUI pixel/geometry comparisons, all chrome/menus/dialogs/sidebar/tabs/files/previews/editor/settings/status/control states and accessibility, plus an independent interaction matrix and native platform evidence. Control dynamic content with fixtures; do not mask required content. Earlier GPUI 18/25 unit and native checks are progress, not pixel-perfect acceptance. Parent coordinates baseline capture and Eva's exact UI rework.

## Current integration checkpoint, not accepted

Latest phase decision: Root authorized Maurice's private G001 metadata-getter implementation and tests after reading PLAN001, SHA `fba3b1230ea7495ef76c3e5e46134f0f423f5fb5c2f342e0ab489ab8a18475e5`. It is a new opt-in candidate after exact P1/P2/P3+008, with default symbol/header absence, no allocation/source mutation, and actual work/ABI proofs still required. This does not authorize installed ABI, constructors, consumers, decoder, fresh factory, codec or defaults. The unrelated SDK-G3 native capture plan is blocked; Root's request for a read-only official Microsoft documentation lookup remains unanswered.

Latest scoped closures: the existing publication allocation-order fix passed nine focused tests and independent source review, without implementing original-slot preparation. Private P4-008's ISO-entry persistent refusal and actual all-screen ISO guard are independently reviewed; measured ISO/source mismatches remain, but the demonstrated false admissions are closed. Native0 is unchanged. The wire/format3 and native integration contracts each have a separate R003 documentation-only revision authorized. Neither grants code/default/runtime acceptance. Current consolidated details are retained in `tmp/migration-current-status.md`.

The workspace has 17 members. Desktop, CLI and remote share the Rust Service; mobile remains remote-only. The latest continuation results below supersede only matching older statuses. None establishes complete application or supported-platform acceptance.

- Private P4-006 replaces P4 after exact Native002 P1/P2/P3. Patch SHA-256 is `a5c189ee81c4f3e7bd7881c8d758d88e3ef7f62914a67a7f3de999e6906fd103`. Reagan's independent source/hash/artifact review closed the remaining DECSTR native-URI false-admission finding. It found no new confirmed blocker in the strict 005-to-006 delta. Saved evidence records ReleaseSafe 522 passes and one skip, targeted Debug 91 passes, and 58 matching source cuts. The review did not rerun them. Native0 is unchanged. New negative-006 evidence confirms persistent DECALN divergence: 12 cases/45 cuts include 27 mismatching admitted cuts, 18 with coherent CURRENT but wrong body. Root verified receipt `aa0c97abec37a8ccf32374ba1b5a83a28fe2b2ed335a5581d2e3265ab728c5f0` and authorized the private 007 DECALN-entry unknown latch. Immutable 007 patch `2744789f01fe726d01712f3de517787c89493abe687502781b21494691dfaa65` now has independent source/hash/artifact review closing that mechanism. Its 28 measured mismatches remain, but false admissions are zero in the same corpus. This is refusal, not source-equivalent DECALN; frozen 006 is untouched. Joint contract R001 also requires C1-C4 corrections for lifecycle, ALT geometry, single shift and active-auxiliary authority. Public C/getter/install/format3, general Legacy, platform and default activation remain held.
- The test-only Windows anchor now records ten kernel/lifecycle passes and strict Files Clippy. Independent review closed the original ledger/adoption/reopen gaps, not production promotion. The unwired multi-component candidate and original prototype passed 16 Windows tests. The private metadata subtree passed 19 cases plus an isolated poison child; Darwin checks are compile/lint only. Independent candidate review and the full publication-owner/per-use writer remain pending. Its fixed issuer-wide directory domain must fault after retained uncertainty and seal before preservation releases pins. The infallible keeper stays separate from admission. General ApprovedRoot remains scope authority, not continuing privacy. Production routing and the current 4-pass/5-failure private composition result are unchanged.
- Native local availability uses typed LocalOnly before optional remote IO, then ReadyLocalOnly only after the same acquired owner starts the local Host. Stored native approvals cannot activate grants while durable writes/revocation are unavailable. Fresh combined Remote/Service/Desktop/CLI test metadata and strict Clippy passed in 5.74s and 7.36s using the exact cached environment. Normal EXE/DLL hashes, sizes and mtimes were unchanged. Logs use `tmp/w-combined-exactenv-20261115-002145`. These were not test executions or Service-lib relinks.
- Transport foundation F001 and format-only F002 are independently source-reviewed, with no confirmed custody or false-NoOwner blocker in that slice. Default/all-feature metadata and strict Clippy passed on Windows, Linux x64/arm64 and Darwin arm64; Android arm64 outbound defaults also passed. Eight unit and five integration bodies were compiled, never executed. iOS std is unavailable. The separate Files/Service/endpoint/driver contract is still blocked on typed binding, receipt borrowing, original-slot capture and Stop/discovery separation. Post-F002 combined Remote/Service/Desktop/CLI metadata and strict Clippy passed in 11.99s/11.52s, including compatibility consumers. Normal binaries stayed unchanged; logs use `tmp/w-post-f002-20261115-021712`. No tests or applications ran.
- Central Service grant custody and atomic provisional adoption are implemented. The standalone mechanical grant harness passed eight cases. Actual native controller acceptance remains pending; no parallel manager receiver, synthetic lease or generic retry is permitted.
- A read-only endpoint audit found an original-owner capture gap. BootGuard retains the attempt after by-value prepare returns, but native creation initially captures into local boxes. Returned errors retain custody; unwind before return can bypass the persistent slot. Attempted-with-no-receipt must be reported as custody unknown, not artifact absence. Native startup acceptance is blocked pending the shared prepare-into-original-owner publication hook.
- The PathBound Desktop editor route was also found to reach V1 journal initialization despite the Native-relative gate. The typed, pre-IO recovery-authority safety gate passed two focused hashed-bin tests, metadata and strict Clippy. It keeps the same Host/UI but disables Files editing. Private-root verification alone does not authorize user-stage recovery. Compatibility APIs are not an activation fallback.

The following older checkpoints retain their original revision and scope.

- The last whole hashed Desktop `--bin` checkpoint passed 300 tests, with one default-ignored entrypoint. It predates private-creation integration. Current production check and strict Clippy pass, but private composition has 4 passes and 5 AccessDenied failures at initial UI-root creation; functional readiness remains blocked. Root uses the actual owner-bound `terminal_profiles()` catalog and explicit unavailable replies. Options parsing is planning-only. Frontend path validation precedes frontend directory creation. Native cold-root creation and endpoint integration are metadata-compiled, but runtime security validation remains open. Native persisted approvals are inventory-only while the durable write/revocation protocol is gated; valid stored V2 records must not activate authentication grants. Normal desktop EXE/DLL hashes, sizes and modification times stayed unchanged.
- Exact UI acceptance remains open. There are 25 actual Electron reference states and 50 independently reproduced zero-pixel comparisons. GPUI has no strict pixel acceptance. Visible automation remains paused; protected user processes 9484, 25904 and 38832 must not be inspected or controlled. Use private artifacts and hashed bin unit tests, never replace the loaded normal executable or DLL.
- External launch uses one prestarted AppHost, the shared local-only namespace, native registration and actual app/window callbacks. Typed Occupied permits handoff; failed owners remain retained. Existing attach-existing flags only hand off launch intents to a registered desktop. Rendering against a standalone headless owner still needs the reviewed native-attachment design and platform ownership proof.
- Raw automation launches ordinary binary pipes, with the sole display parser ready before target launch. Show atomically publishes reserved Host IDs and transfers that parser, not a replay-tail replacement. The successful owned automation group passed 10 tests. `LegacyAutomationV1` matches Node's per-stream decoding and callback CR/LF normalization, including all 65,536 interleaved byte pairs. Process cleanup, source finish, parser retirement and final-view consumption remain separate proofs.
- Runtime's latest full all-target run passed 164 tests and strict Clippy, including grant/recovery failure contracts. Actual owned Windows PTYs exercised controller recovery stamps, byte and side-effect gates, release/takeover without process kill, binary NUL/high bytes, and Ghostty-encoded X10/SGR mouse input. V1 epochs and observer behavior remain unchanged. Native Linux/macOS acceptance is not inferred from these Windows tests.
- Approved devices now receive fixed Write and explicit `ApprovedFullAccount` filesystem authority under the host account. The post-flip files all-target checkpoint passed 90 test executions, with one explicit Windows 8.3 prerequisite ignore. Two later private Windows anchor tests passed separately; neither count establishes ReFS or platform acceptance. Strict known-version approval migration and peer-ID-only native approval are implemented. Host `CapturedSettlement::Unavailable` never resumes or changes a settlement marker; full Host tests and Clippy passed. Positive actual input still uses the existing pending activity flush.
- The unshipped observer-v2 and manager-v2 profiles are removed. V1 is the only remote protocol.
- The asset-backed desktop editor supports 8 MiB through the stable app-scoped chunk/CAS backend. Its 27 focused tests passed. Generic field and inline limits are unchanged. Full large-document native accessibility is still unavailable; those documents expose an unavailable Group rather than partial full-text semantics.
- Experimental format2 P5 composes exact Native002 patches 1–3 and corrected P4-002. The revised immutable experimental-002 candidate reports 496 passed and two intentional skips, the 38-case/214-cut matrix, refusal builds and experimental/default-OFF public C checks. Independent static and artifact review closed the four original findings in this candidate. That review did not rebuild it or establish broader property, renderer or platform acceptance. Public Legacy construction and application adoption remain off. Shipped xterm DOM fixtures show zero-width and orphan-wide row flow differs from existing physical-grid renderers; a checked versioned render contract is required.
- iOS, native Unix/WSL, mobile whole-app adoption, live OS accessibility, privileged effects, packaging/legal gates and performance acceptance remain open. Android runtime testing remains on hold without supported startup network isolation. Do not relabel immutable artifacts or prototype tests as live-HEAD or application acceptance.

Current ownership is Walter for thin Service/CLI/native hooks, Musa for the authenticated remote backend and approval policy, Russell for canonical protocol/client, Eirik for Runtime, Kanishka for Host, Eva for Root/composition, Andre for Settings, Tughril for native boot security, Electra for the editor, Bai for private large-document accessibility, Cora/Maurice for the Legacy render contract, Brooklyn for format2, and Heather for build/dependency composition. Mobile consumers remain coordinated under Lakshmana, Carter, Shakuntala and Hafsa. Keep file ownership explicit and one Runtime event consumer.

## Approved device access policy

The user chose one-click approval with terminal control and full filesystem access under the host's OS account. Do not add device-level view-only, selected-folder or No-files choices. Authentication, pending approval, revocation, terminal ownership leases, stale-generation checks and OS access failures still apply. Approval must never elevate the host account.

The server must mint an explicit FullAccount file capability. Empty root lists, guessed drive roots and client-provided flags are not authority. Migrate only validated known approval records; pending, revoked, malformed and unknown future records must not become approved. Keep v1 wire schemas unchanged.

The existing elevated/admin-source local-only restriction stays in place pending the user's answer to the follow-up question. This is separate from ordinary full host-account file access and must not become an implicit UAC or sudo operation.

## Architecture to build

```text
GPUI desktop                    React Native mobile
  Rust UI                         TypeScript presentation
  custom terminal element         native terminal component
           |                               |
           +------ shared Rust client -----+
                          |
                  validated host contracts
                          |
             Rust desktop/headless host
             sessions / SQLite / files / PTYs
                          |
                owned shell and processes

Terminal views and host screen mirror use a safe Rust wrapper
around the same pinned libghostty-vt version.

Embedded Tailscale remains in Go behind a narrow transport contract.
```

Keep GPUI, mobile bindings, and OS integration out of the domain/core crate. Share domain types, validation, client behavior, and terminal algorithms, not framework widgets. Share only appropriate code with mobile; it is currently a remote-only client and must not acquire local host/PTY capabilities by accident.

Prefer a small Cargo workspace with focused crates for domain/contracts, Ghostty wrapper, terminal/input, host, CLI, desktop, and mobile bindings as needed. Establish real dependency boundaries before adding crates; do not create empty architectural layers. Keep files under 350 lines unless a split would clearly hurt cohesion and the exception is explained.

## Upstream facts and constraints

Verified during planning, but re-check pinned versions before implementation:

- Ghostty is primarily Zig, not Rust. Rust calls the C ABI. The build still needs a pinned Zig version or verified prebuilt artifacts.
- `vt-1.0.1` exists. Its documentation promises a stable C ABI across 1.x.
- Use `include/ghostty/vt.h` and the documented public headers.
- Do NOT build on the internal `include/ghostty.h` surface/app embedding API. Its header explicitly directs external consumers to libghostty-vt.
- libghostty-vt provides parsing, terminal state, reflow, scrollback, render-state inspection, selection/search facilities, input encoding, paste, and effects callbacks.
- It does not provide a renderer, windowing, PTYs, session management, or the application UI.
- Snapshot bytes have a separate versioning policy and are NOT covered by C ABI stability.
- Upstream lists Linux x64/arm64, macOS 13+, iOS 13+, Android, Windows x64 GNU ABI, and browser WASM as built/tested. Windows arm64 GNU ABI is built but not tested. Verify compatibility against Shellfox's actual minimum OS versions.
- GPUI is evolving pre-1.0 software. Pin its version/revision and keep its types inside UI adapters. Windows, Linux, and macOS acceptance is mandatory.

Primary references:

- https://github.com/ghostty-org/ghostty
- https://github.com/ghostty-org/ghostty/blob/vt-1.0.1/PACKAGING-LIBGHOSTTY.md
- https://github.com/ghostty-org/ghostty/blob/vt-1.0.1/dist/doxygen/pages/stability.md
- https://github.com/ghostty-org/ghostty/blob/vt-1.0.1/include/ghostty/vt/snapshot.h
- https://github.com/ghostty-org/ghostling
- https://github.com/zed-industries/zed/tree/main/crates/gpui
- https://github.com/xtermjs/xterm.js

Ghostling is a minimal example, not production terminal code to adopt wholesale. Audit all reused code and bundled dependency licenses. Ghostty and xterm.js are MIT-licensed; retain required notices for copied/adapted code and tests, including Rust translations. Check GPUI and dependencies independently. Do not assume all code in the wider Zed repository has GPUI's licensing terms.

## Current code map

Start from current code and tests, not historical backend documentation.

- `package.json`, `mobile/package.json`: runtime dependencies and verification commands.
- `src/host/`: shared host construction, RPC, resources, automation launch.
- `src/main/terminal/backend.ts`: PTY boundary and process integration.
- `src/main/terminal/screen-mirror.ts`: authoritative host-side headless xterm state, query replies, titles, OSC 7 directories, ANSI screen restore.
- `src/main/terminal/control.ts`, `delivery.ts`: ownership epochs, ordered delivery, replay and credit.
- `src/main/terminal/service.ts`: terminal/session operations and lifecycle.
- `src/main/repository.ts`: SQLite schema, migrations, metadata.
- `src/renderer/terminal-surface.ts`, `terminal-client.ts`: desktop xterm presentation and client boundary.
- `src/renderer/terminal-queries.ts`, `terminal-links.ts`: terminal-specific client behavior.
- `mobile/src/terminal/terminal-page.ts`, `native-text-input.ts`, `page-keyboard.ts`, `TerminalView.tsx`, `useStickyModifiers.ts`: mobile terminal, input and bridge.
- `src/main/remote/`, `src/main/files/`, `src/main/ssh/`: substantial existing features beyond terminal emulation.
- `src/main/terminal/windows-elevated.ts`, `tracking.ts`, `unix-close.ts`: platform lifecycle responsibilities.
- `native/tailscale/`, `native/mobile/`: active Go transport integration.
- `native/Shellfox.ElevatedTerminal/`: active Windows elevation helper.
- `scripts/terminal-native.mjs`: currently shipped native terminal integration.
- `docs/platform-support.md`, `docs/mobile.md`, `docs/headless.md`, `docs/cli.md`, `docs/files-api.md`, `docs/terminal-fixes-verification.md`: behavior and evidence.

Older `Shellfox.Native`/`Shellfox.Linux` projects and some native docs describe a retired external-window backend. Trace current build/runtime reachability before porting anything. Do not port dead code merely because it exists.

## Baseline inventory and implementation decisions

See [the baseline inventory](docs/ghostty-rust-inventory.md) for the feature/test map, exact focused verification command, dependency evidence and next-worker scope. See [the runnable GPUI terminal guide](docs/gpui-terminal-experiment.md) for fixture/shell/smoke commands, isolated paths and current limitations. This section records observed progress, not milestone acceptance.

### Milestone 0 status: partial

- Initial inspection confirmed `experiment/ghostty-rust` at `c8cafb8`, with only this plan untracked and no Rust workspace. Implementation workers subsequently started the Cargo workspace. Do not overwrite their files from an earlier inventory snapshot.
- Read-only inspection of `../shellfox` confirmed 20 tracked changed mobile/docs paths plus untracked `SwipeView.tsx` and its test. No changes were imported, committed, reverted or stashed. The experiment's mobile baseline remains the committed version.
- The inventory maps active host/terminal, delivery/recovery, storage, remote/files, CLI/automation, SSH, OS integration, mobile and update/packaging behavior to existing code and tests. Individual Rust replacements have not passed parity acceptance.
- The host is Windows x64. Inventory found Node 24.15.0, pnpm 12.9.1, .NET SDK 8.0.204 and MSVC tools. Go and Zig were absent from PATH. Rust 1.97.1 is installed alongside default 1.85.1 and stable 1.99.1. Use a worktree toolchain pin, not a global default change.
- Retained Windows x64/source/dependency artifacts pass verification. The isolated Windows experiment's paths and owned-process cleanup now have scoped runtime/review evidence below. Still outstanding: remaining-target artifact verification, full dependency-license audit, effective platform/graphics minimums and platform-native acceptance. Milestone 0 remains partial.

### Dependency and lifecycle decisions

- Pin Ghostty `vt-1.0.1`, peeled commit `108cacb17ed8990840cd14abfce43d8ed8503ca1`. Source builds require Zig 0.17.x, minimum 0.17.0. Use the official matching source/dependency tarballs or verified prebuilt artifacts, not a moving Git branch.
- Windows uses the official GNU-ABI DLL through `ghostty-vt.lib`, which upstream explicitly supports for MSVC consumers. Do not call this a known ABI blocker or assume static GNU archive compatibility. Independent final review reports six real-library wrapper integration tests and wrapper Clippy passing under Rust 1.97.1 MSVC after the poisoned-state fix. Detailed evidence and limits are recorded below; this is not a completed application gate.
- The Windows x64 ZIP SHA-256 is `8e2eb9e1822e85d786b456c8beb86f49d09c318c91f0c2913f91e7ffa12cda29`. The retained x64 ZIP and matching source/dependency tarballs now pass hash checks against the authenticated upstream minisign manifest. Full pin/checksum/source evidence is in the inventory. Windows arm64/Apple artifacts and native builds on other targets are not covered by this result.
- Implementation workers selected published `gpui = "=0.2.2"`, `portable-pty = "=0.9.0"` and Rust 1.97.1. GPUI's registry checksum is `979b45cfa6ec723b6f42330915a1b3769b930d02b2d505f9697f8ca602bee707`. Validate against the published crate, not current Zed examples. Current Zed HEAD also says 0.2.2 but uses a different API/workspace and Rust 1.98.1.
- `portable-pty` direct-child kill does not meet the cleanup requirements. The reviewed Windows path starts only the trusted waiting supervisor, assigns its original process handle to a non-breakaway, kill-on-close Job, verifies exact Job membership, then releases GO to allow shell creation. READY gates public input. Shutdown verifies the original root and empty Job; failed cleanup retains ownership. Linux/macOS shell launch is refused before PTY allocation until equivalent identity/supervisor guarantees exist. No numeric-PID fallback and no blanket prototype exemption.
- A fixture-driven GPUI terminal can provide a runnable rendering/input check while shell launch is gated. It does not pass milestone 1. Real shell, representative TUI and safe cleanup remain mandatory.
- Existing Android minSdk is 24; iOS project says 15.1 while its Podfile delegates to React Native's effective minimum. Reconcile on macOS. Existing desktop/headless architecture targets remain in scope. GPUI GPU/driver and effective OS minima are not yet verified. Do not infer application support from Ghostty's library matrix.

### Baseline verification evidence

Historical results below are retained. [Later attributed results](docs/ghostty-rust-worklog.md) supersede current missing-helper status: Juan's builds/scoped tests passed; Salahuddin's subsequent full `pnpm test:unit` passed 338 files/4039 tests with 59 skips after the helper fixes. Packaging has 8 unit passes. Preserve the independently reproduced file-write ordering flake and earlier worker-exit history; no hidden waiver or assertion fix is claimed. Do not repeat full-suite/typecheck runs merely to confirm these handoffs.

- `pnpm typecheck`, 150-second outer timeout: passed, exit 0, including mobile typechecking and existing generated terminal HTML.
- `pnpm test:unit`, 150-second outer timeout: timed out, with prerequisite failures already visible. No pass claim.
- A full run already launched before the instruction to avoid more full reruns completed as `pnpm test:unit --reporter=dot --no-color`, 540-second outer timeout: exit 1 after 206.85 seconds. Files: 323 passed, 14 failed, 3 skipped. Tests: 3953 passed, 60 failed, 80 skipped. Inventory made no further full rerun.
- Fifteen focused terminal/backend, query/recovery, delivery/client and mobile text-input/load suites passed: 194 tests, exit 0. Exact command is in the inventory.
- `pnpm exec vitest run src/host/automation.test.ts src/host/rpc.test.ts --reporter=dot --no-color`: 19 failed, 1 passed, 3 skipped. Failures identify the missing active C# helper and unavailable Windows private-permissions helper. Failures in the inventory's completed full run reported these prerequisites.
- A separate independent reviewer full-suite run failed with different results: 15 failed, 321 passed, 3 skipped files; 61 failed, 3947 passed, 80 skipped tests; and one unexpected Vitest worker exit. Most failures concerned the unavailable control-endpoint permissions helper, but `src/main/files/service.test.ts` also had an authorization-race assertion expecting `first` and receiving `second`. Juan's later repro identifies a flaky queue-admission-order assertion with one success, one conflict and unauthorized writes denied; no fix was made. The unexpected worker exit remains unverified. Existing TypeScript source was not changed, but this does not establish that every failure is a prerequisite issue or prove absence of regressions. Do not rerun the full suite again in this pass.
- `src/main/control-endpoint.ts` calls the Go Tailscale helper's `--private-permissions` mode. Juan now reports both `pnpm build:tailscale` with project-scoped Go 1.27.3 and the active C# helper build passed, with built helpers retained. The recorded prerequisite failures are historical. Do not bypass permission validation or rerun the full suite without coordination.
- `pnpm test:native`, 180-second outer timeout: exit 0 in 12.5 seconds. The .NET suite restored, built and passed 79 tests with no skips. The following five Node pipe-run tests were all skipped because the active `Shellfox.ElevatedTerminal.exe` prerequisite is missing. This does not prove active-helper descendant cleanup or fix the earlier baseline prerequisite failures. Tests used isolated fixtures; no production data or application processes were used.
- Inventory did not build the active helper/Go prerequisites or run real PTY/storage/GUI/packaged/mobile acceptance or release benchmarks. No lint script is configured in the baseline package. Windows arm64, macOS, Linux, Android and iOS native acceptance remain unverified by this inventory.

### Wrapper integration review evidence

The independent reviewer reported no blocking wrapper issues after the poisoned-state fix and checked ABI declarations, ownership, thread restrictions and cleanup. These results were relayed by the integration lead, not rerun by the inventory worker:

- `cargo test --locked -p shellfox-ghostty-vt --test terminal`: six integration tests passed against the real Ghostty library.
- `cargo clippy --locked -p shellfox-ghostty-vt --all-targets -- -D warnings`: passed.
- The build worker additionally reports `cargo test --locked --offline -p shellfox-ghostty-vt -p shellfox-ghostty-vt-sys --all-targets` passed 17 tests: six ABI/wrapper integration tests and 11 build-support tests. Scoped wrapper/sys Clippy with warnings denied also passed.
- The build worker reports six release-build wrapper integration tests passed. This is release ABI/loading coverage, not a release application benchmark.
- `pnpm typecheck`: passed on the reviewed state.
- An independent 200,000-request device-attributes query storm confirmed that callback overflow leaves the terminal poisoned. Subsequent feed, render, reply-drain, key and paste operations all returned `Err(-6)`, resource limit exceeded, rather than silently continuing. The caller must stop its PTY on this error.
- Reviewer scratch artifacts were cleaned.

This proves only the reviewed wrapper and official Windows GNU DLL/import-library route on Windows MSVC. It does not prove a contained PTY, real-shell/TUI operation, native AltGr or IME handling, other native OS targets, or allocation-failure handling under fault injection. Allocation-failure injection was not performed. Milestone 1 remains open.

### Artifact verification evidence

The inventory worker ran the new build-support checks after integration:

- `node crates/ghostty-vt-sys/build-support/prepare.mjs verify`, 60-second outer timeout: passed. Retained Windows x64 ZIP and matching Ghostty 1.0.1 source/dependency tarballs match pinned hashes and the authenticated upstream minisign checksum manifest.
- `node --test crates/ghostty-vt-sys/build-support/prepare.test.mjs`, in the same bounded invocation: six tests passed. Checks cover retained pins, corrupted-archive rejection, manifest/trusted-comment authentication, exact Zig 0.17.0 selection, offline/fresh-cache build recipes and target/deployment-option validation. This does not prove a source build or another target's runtime.
- Windows desktop compile, runtime and containment results are recorded separately below. These archive checks alone do not prove a runnable terminal.

The verified Windows x64 ZIP and matching source/dependency archives, 16.46 MiB total as reported by the build worker, are intentionally retained in `crates/ghostty-vt-sys/build-support/vendor/`. Provenance and required Ghostty/dependency notices are documented in [the build guide](crates/ghostty-vt-sys/README.md); they are build inputs, not disposable scratch. The source recipe pins exact Zig 0.17.0 for reproducibility, stricter than upstream's 0.17.x compatibility rule. No Zig source build was run. Its default Android API 26 recipe still needs reconciliation with the app's minSdk 24 before mobile adoption. No minSdk change or support removal is authorized.

`.github/workflows/ghostty-experiment.yml` configures Windows x86_64 MSVC artifact verification, formatting, wrapper/sys tests, release ABI loading, scoped Clippy and desktop compilation. It has not been executed remotely and is not other-platform CI evidence. No release was triggered. Final local workspace formatting and Clippy results are recorded below.

### Milestone 1 status: partial Windows runnable slice

Final independent review found no blocking cleanup/query defect in the isolated Windows x64 slice after both final-output draining fixes. This is scoped signoff, not milestone acceptance. Results below were relayed by the integration lead; the inventory worker did not rerun GUI or PTY tests.

| Check | Final evidence |
| --- | --- |
| `cargo check -p shellfox-desktop --locked` and `cargo build -p shellfox-desktop --locked` | Worker reports successful check and linked debug GPUI executable under Rust 1.97.1 MSVC |
| Native fixture and `--shell` launches | Worker separately launched each owned process, observed a native window titled `Shellfox GPUI experiment`, then closed it without a forced kill. The shell launch exercised GPUI and PTY together. Fixture close exited within 10 seconds |
| `node scripts/gpui-experiment.mjs smoke` | Worker and independent reviewer passed real system `cmd.exe` output through the owned PTY into a Ghostty marker row, Ghostty-encoded paste/Enter, resize to 100x30 and Job cleanup |
| `cargo test -p shellfox-desktop --locked` | Latest independent run passed 11 desktop tests |
| `cargo test -p shellfox-terminal --all-targets --features test-support --locked` | Final worker run passed two unit tests and 10 integration tests. Independent review confirmed the 10 Windows lifecycle tests, including missing-GO timeout, retained ownership, descendant cleanup and unrelated-process noninterference |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Final independent run passed |
| `cargo fmt --all -- --check` | Final independent and worker runs passed |

Worker Clippy also passed for all desktop/terminal targets with terminal `test-support` enabled. The worker reports the linked build, final smoke, launcher syntax and diff checks passed; authored desktop files are at most 338 lines and PTY files at most 234. Worker scratch was cleaned. `pnpm typecheck` passed; the full unit suite was not green at this slice, before Salahuddin's later passing run. Existing `src/`, `mobile/`, `native/`, package manifests, pnpm lockfile and `AGENTS.md` were not edited in this slice.

The wrapper/sys all-targets result remains its separate 17-test result above. Do not combine it with the 11 desktop or 10 lifecycle tests, or reuse the earlier 9/10 desktop and 9 lifecycle counts as final results.

The reviewed view uses `Active`, `Draining` and `Closed` phases. Natural root exit disables new input but keeps parsing queued output through EOF; retired query destinations receive no replies. Root polling continues in both Active and Draining until Job accounting catches up and console closure finishes. One regression feeds 20 chunks of 4096 bytes followed by DSR/DA and `FINAL_OUTPUT_MARKER`, verifies the marker and rejects retired writes. Another models root exit on tick 1 with Job completion only on tick 3. Reviewer scratch artifacts were cleaned.

Run from the repository root:

```sh
node scripts/gpui-experiment.mjs fixture
node scripts/gpui-experiment.mjs shell
node scripts/gpui-experiment.mjs smoke
```

No mode argument defaults to fixture. `--release` is optional. The launcher uses a new `tmp/gpui-experiment/run-PID-TIMESTAMP` directory per invocation; direct executable use requires an explicit absolute `--data-dir`. Cargo deploys `ghostty-vt.dll` beside the executable with notices, without PATH changes. See [the experiment guide](docs/gpui-terminal-experiment.md) for exact direct commands and isolation/cleanup limits. The smoke route does not initialize GPUI, but still links desktop dependencies; it is not the migrated headless host.

The vendored portable-pty unused inherit-cursor constant and `proc-macro-error2` future-incompatibility notice remain acknowledged upstream warnings, not suppressed Shellfox-code diagnostics. The minimal cursor-inheritance patch, provenance and license are documented in the experiment/PTY guides. Full dependency license/security clearance remains open.

Milestones 0 and 1 remain partial. Unix shells are gated, Windows arm64 is untested, and mobile/headless clients are unmigrated. Native IME, AltGr/dead-key/Option composition, enhanced printable-key metadata and focus ordering are not accepted. Selection/copy, links, mouse reporting and accessibility are missing; glyph rendering still needs optimized batching/shaping. Representative full-screen TUI, backlog-plus-resize ordering, sustained-output response, DPI/GPU matrix and release performance acceptance remain open. No production host/recovery migration or feature-parity acceptance is implied. The earlier failed unit runs remain historical; Salahuddin's later full run passed after Juan's helper fixes. The file-write ordering flake has no claimed fix, and the earlier worker exit has no diagnosed root cause. See the continuation worklog.

## Active application work and ownership

The [continuation inventory](docs/ghostty-rust-inventory.md#full-migration-continuation) defines contracts, reusable protocols, parallel work boundaries, Go transport prerequisites and the complete open acceptance checklist. These assignments authorize implementation, not completion claims.

- Andre owns real `shellfox-core` and `shellfox-host` contracts, validated session metadata/preferences and SQLite tests. Eva consumes that API rather than building another session store. Initial session creation is metadata, not implicit PTY spawn. Generation-aware cleanup prepare/commit is now defined in host source; runtime/UI integration and acceptance remain open.
- The agreed store is `shellfox-rust-v1.sqlite3` in an explicit stable experiment data directory, with application identity and version checks. Disposable smoke directories stay separate. Do not open or migrate production `manager.sqlite3`. Legacy v1-v5 import later uses a consistent copied database, never stored process ownership or automatic shell restart.
- Gavrila owns shell profiles/environment and owned PTY integration. Keep portable profile preferences separate from experiment data roots. Retain the supervisor/Job guarantees; interactive usability must not weaken cleanup.
- Eva owns the actual GPUI session/tab/settings/files workspace and default application route. Fixture mode becomes explicit. Eirik owns `shellfox-runtime`, the single PTY/parser owner with bounded commands/events, leases and replay. Electra owns `shellfox-files` scoped I/O for Eva. Heather owns Ghostty mouse/selection/hyperlink APIs and coordinates root Cargo/lockfile changes. Coordinate shared desktop terminal edits rather than replacing each other's work.
- Mulavarman owns the Go/native transport adapter and Russell owns the shared typed client/state machine, using their agreed protocol/transport boundary. Walter owns the real Rust headless service/CLI, consuming that client rather than bypassing it with database access. Smaller experiment DTOs are not existing desktop/mobile wire compatibility. No GPUI dependency in these shared/headless crates.
- The prototype defaulted to a no-PTY fixture; shell mode used cmd, scratch cwd and a System32-only PATH with redirected home/config. That excluded `ls` as a shell command and installed tools such as `pi`. The single terminal root also had no Shellfox session/settings/files navigation or mouse wiring. Fix the integration, not an `ls` alias or a user-specific executable path. New worker files alone do not prove those gaps fixed.
- Runtime, files, client, headless/CLI and transport work is assigned in parallel. Guarocuya owns new Unix adapter modules, Madelyn SSH private preferences/known-hosts, Salahuddin Rust packaging/resources, Carter the actual terminal Rust bridge/RN specs and Lakshmana its client companion, Shakuntala Android and Hafsa iOS native integration, and Juan Go/C# helper prerequisites/security tests. Native mobile Ghostty views and remaining elevation/WSL/update behavior still need integration and acceptance. See the inventory for exact boundaries. Do not create unused crates, drop a platform or grant mobile local host privileges.
- The lead reports Gavrila's four shell-profile/usability tests and 11 lifecycle tests passed, including pwsh/Windows PowerShell `ls` and `node`/`pi` resolution without running the user's account-bound Pi application. This closes a scoped command-discovery check, not Pi authentication, full TUI, integrated workspace or cross-platform acceptance. Exact commands and remaining gaps are tracked in the inventory.

Milestones 0 and 1 remain partial. Actual protocol/client/cursor, mobile bridges and RN specs exist. Reported passes include core/host 30, client/cursor/wire 26, latest runtime 16 unit plus 6 real integration, Windows files 3 race plus 11 integration, and mobile terminal ABI/owner-thread/queue/reset 8 on Windows. Scoped Clippy, fixtures, codegen and TypeScript results are detailed in the worklog. Arm64 file cross-checks are compilation only, not native acceptance.

That 15-member lock and early UI/mobile test report was a historical continuation checkpoint. The current graph and active blockers are summarized above; detailed earlier evidence remains in the worklog. Full recovery and whole-application acceptance are still open, so retain the existing compatible recovery path until its replacement is proved.

## Non-negotiable behavior

Preserve and test these while changing implementations:

- One actual PTY per owned live terminal; desktop-owned and headless-owned host behavior remain consistent.
- Closing a remote client does not kill host terminals. Host shutdown follows the existing confirmed cleanup policy. No promise of live-shell survival after host restart.
- Exact process identity and ownership checks, stale generation rejection, failed-cleanup ownership retention, and no unsafe numeric-PID fallback.
- Windows normal/elevated shells, WSL distro handling, Linux cleanup, and macOS supervision retain their safety semantics.
- Single controlling view, explicit takeover, epochs, output sequence numbers, ACK credit, bounded replay, resize ordering, and clean disconnect/reconnect.
- Host-side terminal query replies remain authoritative. Historical replay and client mirrors must not generate duplicate PTY replies or replay clipboard side effects.
- Runtime validation at IPC, network, FFI/helper, persistence, and mobile boundaries.
- Database migrations and settings retain existing data. Do not store terminal output, credentials, or process command lines in new persistence by accident.
- Remote approvals, revocation, permissions, transport ownership, upload/path restrictions, and external-link/clipboard policies remain enforced.
- Headless distributions do not depend on GPUI, GPU initialization, Electron, a display server, or mobile runtimes.
- Existing sessions, files/previews/editing/transfers, agent detection, CLI commands, automation, SSH, keep-awake, OS integration, and updates need an explicit feature-parity inventory. No silent disappearance.

## Input design

Use Ghostty's terminal input encoders. Do not duplicate its key/mouse/paste escape-sequence logic by porting xterm's encoder.

Use xterm's relevant behavior, code, and tests as references for normalization, composition, shortcut routing, duplicate suppression, and event ordering. Preserve attribution. Browser textarea workarounds should not be translated line-for-line into native Rust.

Represent physical key events separately from committed text and IME preedit. Preserve release/repeat/modifiers and physical/logical keys without double-sending text. Platform adapters capture events; the shared layer applies Shellfox shortcuts and invokes Ghostty encoding against the correct terminal modes. Never consume Ctrl+C as an application shortcut while a terminal should receive it.

Validate international layouts, AltGr, dead keys, CJK IME, emoji/graphemes, paste, hardware keyboards on mobile, software-keyboard deletion/composition, focus loss, and sticky modifiers. Test the actual UI event source, not only synthesized Rust events.

## Recovery and mixed-client protocol

Current screen restore is an ANSI escape stream, not a Ghostty snapshot. CLI attachment renders into the caller's ordinary terminal. Older desktop/mobile clients may use xterm.

The public `vt-1.0.1` formatter cannot export inactive buffers or saved cursors, and pending-wrap reconstruction can overwrite a saved cursor. This is a full-state recovery blocker, not a missing UI feature. Maurice reports 12/12 Ghostty+xterm recovery tests passed and 3 FullState counterexamples. Actual `FormattedActiveBufferV1` includes tested UTF-8/CSI barriers but is not saved-cursor, inactive-buffer or Kitty-mode restoration. Heather owns public formatter/continuation/mode bounds. These passes do not establish missing-state fidelity.

- Retain the existing Node ANSI-compatible recovery path until an equivalent new protocol/full-state path is proved. Do not silently substitute the limited projection or announce a fully translated host/client migration.
- Full-state closure requires an adequate upstream public API or an explicitly negotiated/versioned Ghostty snapshot protocol with tested legacy fallback. No internal ABI, unbounded replay/output capture or output persistence workaround.
- Negotiate terminal capabilities and recovery formats explicitly.
- Do not send Ghostty binary snapshots to xterm or arbitrary terminals.
- Do not assume separately versioned snapshots are interchangeable across library versions. Validate versions and bounds before decoding; define fallback behavior.
- Maintain output sequence/snapshot boundaries, parser continuation, resize ordering, alternate screen, cursor/modes, scrollback, and title/directory behavior.
- Restrict advertised terminal capabilities to what the active client can actually display during mixed-engine migration.
- Test long-running full-screen TUIs, detach/reattach, takeover, lost replay, same-grid repaint, unfinished UTF-8/escape sequences, and slow mobile clients.

## Implementation milestones

### 0. Inventory and reproducible baseline

Map active features to implementation/test files and create a parity checklist. Record baseline toolchain/build/test status and platform evidence. Confirm mobile baseline differences against the original dirty checkout without modifying it. Establish isolated experiment user-data directories and test resources.

Choose exact GPUI/Ghostty/Rust/Zig versions, supported minimum OS versions, and dependency license handling. Keep third-party fetching reproducible with hashes/signatures as appropriate.

### 1. Ghostty wrapper and runnable GPUI terminal

Add the Rust workspace and a narrow safe wrapper owning terminal objects, callback lifetimes, buffers and error handling. Keep unavoidable unsafe FFI contained and documented; do not conceal failures with assertions or unchecked conversions.

Build a real GPUI terminal backed by a real PTY in an isolated experiment app. Implement batched glyph/background rendering, font discovery/fallback/shaping, DPI/scaling, cursor, scrolling, selection/copy/paste, basic links, input, IME and resizing. Ghostty render state is not a font shaper or completed renderer. Separate the renderer from core terminal logic so mobile can reuse appropriate algorithms without depending on GPUI.

Prove the minimum input/graphics/accessibility route on Windows, macOS and Linux. Measure release builds, not debug builds. Provide basic accessible terminal text and control semantics; a GPU texture alone is not accessibility.

Gate: real shell and representative full-screen TUI work, lifecycle cleanup is safe, no duplicate input, rendering remains responsive under output load. Document unverified platforms rather than claiming parity from compilation.

### 2. Shared Rust host and CLI

Start real contracts, isolated SQLite metadata/preferences and session-manager tests now, alongside stage 1 and the GPUI workspace. Preserve `createSession.requestId` idempotency, settings omission semantics and revisioned events. Integrate terminal ownership through prepare/cleanup/commit, not fake running records or UI-owned SQL.

Then port terminal services, control/delivery/recovery, files, remote integration and CLI incrementally. Keep existing clients operational through compatible contracts until replaced. Use differential tests against shared Zod schemas and existing behavior. Keep host tests independent of GPUI. The new store's own schema is not legacy database compatibility; test explicit copied-data import separately.

Reuse Go Tailscale helpers through their existing validated boundary initially. Port active C# and platform helpers only when behavior can be proven equivalent; retaining a helper temporarily is safer than weakening cleanup or elevation guarantees.

Gate: real existing React Native client connects to the Rust host, CLI operations and interactive attachment work, database migration is tested on isolated fixtures, platform lifecycle/remote/recovery tests pass.

### 3. GPUI desktop feature parity

Build the actual Shellfox workspace now, in parallel with shared-host and terminal work. Default launch opens that workspace, with explicit fixture/smoke modes retained for tests. Wire persistent sessions/tabs/settings first, then host-owned terminals and files. Visible controls must invoke real tested behavior.

Replace the desktop UI feature by feature: local/remote machine navigation, session history/pins/settlement, tabs, all settings categories, file browser/previews/editing/transfers, dialogs, status/agent activity, window behavior, notifications and OS integrations. Keep privileged operations in host adapters, not views or portable core.

Gate: pixel-perfect current Electron UI in all controlled matched states, full independent interaction/accessibility parity and native OS evidence under the visual requirements document. Test multi-window and takeover behavior without redesign or reduced UX. Any deviation needs user approval. Remove Electron/React desktop dependencies only after acceptance and no active path needs them; retain React Native dependencies used by mobile.

### 4. React Native bindings and native Ghostty terminal

Retain existing React Native screens and adapt their host/client binding incrementally. Use a narrow typed native interface for the Rust client library. Add a native terminal component for both Android and iOS using supported React Native native-component/module mechanisms for the pinned RN version.

Keep high-volume terminal data and rendering off the JavaScript per-cell/per-frame path. Use a bounded ordered native path with backpressure; send coarse state changes to JS. Choose and validate mobile graphics integration rather than assuming GPUI runs on mobile.

Implement native IME/software keyboard, touch selection, scrollback, sticky modifiers, clipboard, accessibility, safe-area/keyboard resizing, suspend/resume/reconnect, Android back navigation, and hardware keyboard input. Preserve embedded Tailscale native behavior.

Keep the existing WebView/xterm path only as an explicit transition mechanism. Remove it after both native mobile implementations and recovery negotiation pass acceptance, not merely after Android compiles.

### 5. Packaging, cleanup and complete acceptance

Build desktop/headless packages for existing Windows/Linux/macOS architecture variants plus Android. Build and test iOS with macOS/Xcode; do not falsely label configured targets as verified. Validate Windows arm64 explicitly given upstream's lower test tier.

Preserve isolated user-data selection, CLI launchers, updates, signing expectations, and correct resource locations. Ensure no desktop dependencies leak into headless artifacts. Audit third-party notices and release checksums.

Remove retired implementation, dependencies, build scripts and stale docs only after replacements are wired and tested. Deliver an accurate migration/parity report and remaining blockers. Do not publish this experiment without the user's instruction.

## Verification requirements

Run existing checks while their implementation remains active:

- `pnpm typecheck`
- `pnpm test:unit`
- Relevant native tests through `pnpm test:native` and active helper-specific checks.
- Relevant terminal, PTY, storage, CLI/headless, remote, mobile, Playwright and packaging checks from package scripts and docs.

Add Rust checks such as workspace formatting, Clippy, unit/integration/contract tests and dependency/license checks, scoped appropriately to avoid forcing GUI initialization in headless tests. Provide repeatable new commands and CI coverage for the new architecture. Do not delete old tests merely to make the rewrite pass; port behavior coverage as implementation changes.

Use isolated fixtures and owned processes. Do not stop existing Shellfox sessions or bind experiments to the installed application's production host/database. Tests requiring real macOS/iOS/Android/Windows/Linux environments must be reported as blocked if prerequisites are unavailable.

Measure idle CPU, memory, startup, typing responsiveness, sustained-output throughput, scrolling, slow-consumer behavior and mobile foreground/background behavior. Use the same workload, terminal dimensions and release build settings for comparisons. No unsupported speed or memory claims.

Each milestone leaves a runnable app, updated parity checklist and scoped evidence. Final acceptance requires pixel-perfect Electron desktop states, full functionality, accessibility and native supported-platform/client proof, not a compile, screenshot or one platform's demo. Deviations require explicit user approval.

## Next integration and completion rule

Finish the real core/host metadata API, stable isolated persistence and GPUI session workspace in parallel with profiles/environment and terminal interactions. Then connect the host's terminal lifecycle, files, client/control and CLI/transport increments using the inventory's complete checklist. Native platform, mobile, recovery, packaging and performance gates remain required. Keep decisions and evidence current; a shell smoke or screenshot is never full migration acceptance.
