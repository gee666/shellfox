# Observer-v2 Android source integration

This is an additive implementation with a separate isolated compile/link/JVM gate. It is not an enabled production route or adoption claim. The wire contract is `crates/shellfox-protocol/observer-v2.md`; its 24 golden JSON fixtures remain the authority. V1 defaults, C ABI symbols and layouts are unchanged. Observer extension ABI 1 uses wire version 2.

## Native API and ownership

The existing `NativeShellfoxRustClient` codegen module gains the exact methods in Carter's spec: `observerV2AbiVersion`, `clientProfile`, `createObserverV2Client`, `discoverObserverV2`, `pairObserverV2`, `observerV2Operation`, `pollObserverV2`, `reconnectObserverV2` and `disconnectObserverV2`. No second module is registered. Observer creation does not discover, pair or fall back automatically.

`clientProfile` returns only the native profile and available API bits. Profile 1 has bits 7; observer profile 2 has delivery bit 1. These are not current approval, input ownership or layout grants. V1 operations still require the Rust-owned lease. An observer has no input/layout API even if a caller sets the view's read-only prop incorrectly.

JNI queries `sfm_client_profile` using client handles, never terminal handles. Binding, reset ticket/grid/done, recovery readiness and consumption call the actual profile's C ABI functions. Dispatch/cancel/read-event/close remain common. `closeClient` is the client lifecycle; `closeTerminal` remains an owner-thread engine operation.

The process HandlerThread still owns parser creation, pumping, frame access, configuration and cleanup. Observer reset uses the current ticket's canonical host grid. Recovery/data ACK preparation follows actual native parser consumption, not receipt or JS assertions. The Rust cursor owns distinct recovery/data ACKs and their stamps. Caller-supplied ACK operations remain denied.

A fixed-grid observer never fits or resizes its parser on a view or font change. Canvas clips/pads the canonical frame locally. Native input connections, keyboard routing, mouse reports and host resize are blocked by the observer profile. Native reset remains owner-authorized. There is no conversion to v1 Owned/Blocked DTOs, fake control epoch, permission or creation time.

## HTTP status and Go transport

The common dispatch wrapper queries the immutable client profile before callbacks. V1 retains its existing `request` callback and `sfm_client_complete`. Observer handles select `requestHttp`, even when the shared client legitimately probes `/v1/discover` for a visible legacy offer. A path prefix never chooses the client profile.

Observer requests call the new immutable Go `RequestWithStatus`. The reviewed gomobile generator maps its methods to Java `requestWithStatus`, `statusCode()` and `body()`. Native validates a received status in 100..599 and forwards it unchanged to `sfm_observer_v2_complete_http`. Non-2xx bodies must be empty. Transport/deadline/cancel failures use status zero and empty bytes. No error message, business UNSUPPORTED response or authentication failure becomes a synthetic 404.

Only Rust's pending DiscoverV2 negotiation may interpret an actual HTTP 404 as a legacy offer. The platform never pairs a v1 client as a fallback. A caller must choose a separate legacy client explicitly.

Both transports use the same exact ten-path classifier. Only `/v1/events` and `/v2/events` occupy the bounded poll pool. Pair and disconnect in either version use finite background completion; idle polls do not hold a wake lock. Lookalike paths, URLs and suffix/query aliases are rejected before admission. Go remains the only outbound carrier, with no caller headers, host secret or local PTY.

## Source gates

```sh
node --test scripts/mobile-native.test.mjs mobile/android/tests/observer-source.test.mjs
pnpm typecheck
pnpm exec vitest run mobile/src/rust-client
```

The 12 Node checks are source-role/build-plan checks, not JNI/runtime tests. Root/mobile typechecks and 46 Rust-client TypeScript tests passed at this source checkpoint. Added JVM policy tests cover exact routes, HTTP status preservation, finite pair/disconnect behavior and profile capabilities. They need coherent rebuilt bindings before the full Android source graph can compile.

The old staged Go AAR does not contain `RequestWithStatus` and the old Rust archives do not contain observer functions. Do not generate fake Java methods, use reflection fallbacks or relabel those artifacts. The isolated build below uses only its own new outputs. iOS source coordination is separate; Xcode/runtime evidence requires macOS.

### Isolated compiled gate

Built from immutable quiet snapshot `860d3aa1c8f4a5f5c234a4058156cc77e1da54e87b9251a8ba3de14595f8a50f`, not live HEAD. The authoritative `snapshot.json` records source fingerprint `0f3329a557395efb09ce0db69a981cf9d28e253965e63b4eaf4e25818fe11c32` and separate Rust, Go and Android/spec input hashes. Profile stays observer extension ABI 1/wire 2 with DEFAULT VT 1.0.1; Native002 was not selected.

All outputs are below `tmp/observer-v2-artifacts/<snapshot>/`. The fresh Go AAR is `gomobile/android/shellfox-mobile.aar`, SHA256 `090bf7a51181972b94920bbb0c31db97f0067a7279a4eed526da2ce4839eb8ad`. Actual javap output confirms legacy `request` and new immutable `requestWithStatus`/`statusCode()`/`body()`. Both Go ELF images have 16-KiB PT_LOAD alignment. Dependency notices and build/tool/flag provenance are beside the AAR.

The private Android working copy preserves all captured source bytes. Native Gradle/codegen dependency sources were materialized from existing cached packages without their old build outputs. Go used offline modules, GOMAXPROCS 1 and package parallelism 1. JNI ABI links ran sequentially. Gradle used offline mode, one worker, in-process Kotlin and private Ninja compile/link pools of one. No dependency or tool download was allowed.

Full JNI shared-library links passed for all four API-24 ABIs against the new Rust archives. Actual Fabric appmodules/JNI Gradle image links passed for the two configured app ABIs. Thirty-four JVM tests passed without failures/skips; lint passed with zero errors and 32 existing template/resource warnings. A first Gradle native attempt exceeded Windows compiler-identification path limits. Its failed log was retained; only private CMake staging directories were shortened, without bypassing a compiler check or changing captured source.

`jni/compiled-gate.json` records the resulting library hashes, ELF dependencies/alignment, component input hashes and all gate boundaries. `jni/standalone-provenance.json`, Gradle provenance/logs and copied JVM/lint reports retain detailed evidence. Source-copy and private dependency source hashes matched after builds. All 477 pre-existing V1 artifact files/AAR were rehashed unchanged; neither they nor source snapshot `91ed…04964` were reused as Shellfox implementation inputs or relabeled.

No observer emulator, device, GUI, account, installation, real network or release operation was performed. API-24 target links and ELF 16-KiB alignment are not device/runtime acceptance. Earlier V1 runtime evidence in `RUST_TERMINAL.md` still does not cover observer-v2.
