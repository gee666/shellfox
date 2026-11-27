# iOS Rust terminal

This implementation adds a Fabric terminal and TurboModule to the existing RN 0.88 application. The terminal renders Ghostty through a native view. A Windows source or codegen check is not iOS acceptance.

## Registration and build dependencies

`ShellfoxRust/ShellfoxRustTerminalComponentView.mm` implements the generated `ShellfoxRustTerminalComponentDescriptor`, props, view protocol and event emitter. `ShellfoxRust/ShellfoxRustClient.mm` implements `NativeShellfoxRustClientSpec` and returns `NativeShellfoxRustClientSpecJSI`. One Objective-C implementation satisfies the entire generated protocol. Owner lifetime registration is lazy, since the installed provider generator creates a prototype before RN creates the actual module instance from that class.

The codegen library for `mobile/package.json` and the shared specs is `ShellfoxRustSpecs`, with these mappings:

- `ios.componentProvider.ShellfoxRustTerminal = ShellfoxRustTerminalComponentView`
- `ios.modulesProvider.NativeShellfoxRustClient = ShellfoxRustClient`

These mappings feed the installed RN generators `generateRCTThirdPartyComponents.js` and `generateRCTModuleProviders.js`. The existing `ShellfoxMobile/SceneDelegate.swift` already assigns `RCTAppDependencyProvider` to the RN factory delegate. There is no legacy `RCTViewManager`, component-class crawling or manual component registration.

The Podfile adds the local `ShellfoxRustNative` pod and a generated `ShellfoxRustRuntime` pod. `install_modules_dependencies(s)` supplies the installed RN Fabric, TurboModule and ReactCodegen dependencies. Runtime headers come from the static XCFramework, not checked-in copies. Neither generated RN headers nor fabricated versions of those headers belong in source control.

The installed RN helper returns iOS **15.1** and Xcode **16.1**. The Podfile still uses `min_ios_version_supported`; all existing Xcode deployment settings remain 15.1. The device arm64, simulator arm64 and simulator x86_64 recipes retain that minimum. Catalyst configuration is unchanged.

## Ownership and bounded work

`SFOwner` owns one permanent `NSThread`. A serial GCD queue would not satisfy the Rust ABI's OS-thread identity check. Creation, normal pumping, frames, input, resize, configuration, selection, scrolling, reset and engine close all run on this thread.

The owner admits at most 128 work blocks and executes at most 16 before servicing native clients and engines. Normal rendering work runs at most 60 times per second. Pacing, poll backoff and withdrawal deadlines use monotonic uptime and relative waits, not the adjustable wall clock. Module lifetime tokens have a separate bounded registry, so invalidation does not depend on admitting a final close block into a full work queue. Invalidation unbinds clients and retires engines on the owner.

An owner preadmission FIFO retains only encoded input/typed resize intents that Rust has not accepted. Each encoded packet stores its bytes and Rust-computed `uint8_t` user-input hint together. Copies, BUSY retries and same-hint merges preserve both; mixed-hint batches remain separate and batch admission rolls back on overflow. Ordinary intents are capped at 64 entries/128 KiB; the total cap is 128 entries/160 KiB, with slots and 32 KiB reserved for critical releases. It preserves press, resize, release order across BUSY. Adjacent pending releases may concatenate, and adjacent pending resizes may coalesce, but neither crosses another input intent. Retry happens only after BUSY, never after OK or an accepted mutation's later network failure. Reset, unbind and close discard obsolete intents. This is not a Go request reorder queue; accepted operations disappear into the sole shared Rust mutation FIFO.

Input and resize first drain through a snapshot of the currently accepted FIFO ticket. Later transport submissions cannot extend this barrier. This prevents encoding a key against old terminal modes or resizing ahead of already accepted output. The barrier is bounded by the existing 4 MiB/64-chunk queue. Normal per-tick pumping retains Rust's 512 KiB budget.

Consumers of `readEvent` must validate the current ABI fields, including `internal`, `resetTickets`, `resetGeometry` and `recoveryReady`. A strict shared TypeScript schema missing these fields will reject real events even if older fixtures pass.

The C ABI bounds output to 4 MiB, 64 chunks and 256 KiB per chunk. Rust retains pressure-blocked delivery and owns host cursor, generation, lease and ACK validation. Native code calls `sfm_client_terminal_consumed` on idle ticks too, so a BUSY or failed ACK can retry without new output. A local delivery ticket is never used as a host sequence.

`SFFrame` copies borrowed UTF-8 and cell data inside the synchronous frame callback. A mailbox retains one replaceable pending frame; UIKit retains the displayed frame. A producer copy can exist while the next frame is being built. Native frames reject malformed dimensions and excessive text rather than retaining an unbounded backlog. No frame or cell array crosses JavaScript.

`SFClientStore` drains C events into a queue limited to 64 events and 16 MiB per client. An individual event is limited to 8 MiB plus 64 KiB of metadata. Explicit JS operation completions remain available to their caller. The ABI's `internal` flag identifies native ACK, input, resize and poll completions, including failures. Success-only internal completions and output-only native polls do not reach JS. Connection/error changes, new reset tickets and host control/changed/reattach/exit events do. An explicit JS poll can request the completion of an already running native poll.

Before every bound engine is pumped, the owner queries `sfm_client_terminal_reset_ticket`, then `sfm_client_terminal_reset_grid` with that exact ticket. It resets at the required host attachment grid, never the local viewport. EMPTY skips obsolete reset notifications or retains the existing grid for a fence-only reset. `sfm_client_terminal_reset_done` validates the ticket and host geometry on the same thread. STALE retries the handshake. This prevents old notifications from resetting a recovered parser or replay from being clipped by a smaller phone grid.

The saved viewport stays deferred until pumping/consumption and `sfm_client_terminal_recovery_ready` confirm that all recovery bytes, including pressure-blocked chunks, reached Ghostty at the host grid. Only then does the owner fit locally, and only to a viewport the view has measured: until its first layout the engine keeps the host grid, because fitting a recovered screen to the create placeholder pushes rows above a parked cursor into scrollback for good. Readonly views may fit locally but never send host resize; the first writable resize sends the viewport even when the local engine already has that size. These queries also work when the JS event queue is full. Reset clears the pending native frame and preserves appearance configuration.

Host resize uses `sfm_client_terminal_resize`, not a JS-side generation/control-epoch cache. It enters the same Rust FIFO as writes at the point of native intent admission. There is no separate resize-completion retry queue. Native Ghostty input uses `sfm_terminal_input_ex` and forwards its hint unchanged into `sfm_client_terminal_write_ex` with the Rust-owned lease. Neither UIKit nor JS classifies settlement activity. Release priority only reserves admission capacity; it does not determine the hint. The host meaningful-input policy remains authoritative. Bytes always use their explicit length, including NUL; there is no platform byte conversion or escape fallback. Generic JS operations cannot bypass the encoder with raw terminal writes or forge consumption ACKs. Client close does not close a host terminal.

## Existing embedded Go transport

`SFGoTransport` injects the existing `MobileRequest(peer, path, body, error)` into the real Rust client's dispatch/completion ABI. It does not start another node, copy authentication files, accept host secrets or create local PTYs.

Request callbacks copy all borrowed peer/path/body bytes before returning. Pair/disconnect requests retain the existing finite UIKit background-task window, without claiming indefinite background networking. Native request identity includes client, generation and request ID. There are 12 ordinary request slots and 16 separate long-poll slots, with no unbounded worker backlog. Response limits and logical deadlines are checked before completion. Errors exposed to JS are sanitized; Go error details and response bodies are not logged.

The existing gomobile request cannot abort an individual socket. Cancellation fences the token and suppresses completion, but the physical worker keeps its slot until Go returns. A deadline does not free that slot early. Rust rejects late generation/closed-client completions and retains no callback context.

`closeClient` first calls `sfm_client_disconnect`, continues native dispatch and event consumption until its completion or a two-second deadline, then calls `sfm_client_close`. The Promise resolves after local close, not when disconnect is merely queued. Remote withdrawal is best effort at the deadline; local success does not claim the server received it. Module invalidation uses the same bounded withdrawal path. Late completions remain fenced.

Backgrounding detaches the viewport, discards marked text without committing it and pauses new polling/ordinary dispatch. A closing client may use its finite withdrawal window. The shared TypeScript client lifecycle owns foreground reconnect and reattach through the approved generation/reset handshake. Native code does not start a second pairing attempt. This is foreground-only networking, not a VPN/background-execution claim.

## Rendering, input and user actions

- CoreText shapes rows with font fallback. Shaped glyph clusters use UTF-16-to-engine-cell anchors, including combining text and wide cells. UIKit draws cell backgrounds, truecolor, inverse/faint/invisible text, underline, strike, selection and a steady block cursor. DPI comes from the active drawing context. Cursor shape/blink and image placements are not exposed by the current mobile frame ABI.
- `fontSize` is bounded to 8 through 40 points. `configureTerminal` first passes the strict Rust configuration schema, then updates native cursor, selection and background colors. Parser palette/default colors also use Rust configuration. Theme changes survive reset.
- A native `UITextView` is the keyboard editor: the text since the last Return or key, with marked text, candidates and software-keyboard deletion. After each edit its committed text goes to Rust, which sends the terminal only the change. Marked text stays local. Composition is discarded on detach/background. Marked text is positioned at the native cursor.
- Hardware shortcuts/navigation use physical key actions, modifiers and repeat/release. Ordinary hardware text goes through UIKit and carries key metadata only when associated with a committed text event. Dead keys and AltGr text stay on the composition path. There is no second text send for the same physical key. Focus loss releases tracked keys into the old binding.
- `focused` requests or resigns the native keyboard on the UI thread. UIKit editing stays disabled until the host-grid recovery barrier completes. Fabric reports `bound`, then `ready`/`recovering`, and `closed`; JS special-key controls should use the same readiness signal. `readOnly` prevents editing. Sticky modifier bits are bounded to 0 through 15. `onInput` reports accepted user input, not output or editor resets. JS special-key controls use typed `terminalInput` JSON through the same owner/encoder/lease path, never raw escape strings.
- Pan scrolls engine scrollback, or sends Ghostty-encoded wheel events when application mouse tracking is active. Shift overrides tracking for local scrollback. Tap press/release and wheel batches enter one bounded native write. Selection mode changes pan to range selection; long press selects a word and opens compact actions. Outside selection mode the pan begins only for mostly vertical motion, leaving horizontal swipes to the app's Files navigation. A pan or long press that begins cancels React Native's JS touch by toggling its surface touch handler, as Android does with `notifyNativeGestureStarted`. Leaving selection mode clears it. `onSelection` is explicit user-selected text only, bounded to 64 KiB of UTF-8 on a grapheme boundary.
- Native Copy obtains engine selection text and writes a local-only pasteboard item. Paste reads the pasteboard only after user intent and uses Ghostty's paste encoder. There is no output-driven clipboard write or unsafe multiline-paste bypass.
- Open link queries actual OSC 8 metadata. It permits only HTTP/HTTPS URLs with a host and no credentials/control characters, then asks for confirmation. It does not scan frame text for guessed links.
- VoiceOver can read native visible text, activate the keyboard, scroll a page, copy selection and paste. Terminal text is not automatically announced for every output frame. Actual VoiceOver rotor/navigation and external-keyboard behavior still require device acceptance.

## Build on macOS without signing or installing

From the repository root, with the pinned project Rust toolchain, Zig 0.17.0, Xcode, CocoaPods and the existing Go prerequisites installed:

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
node scripts/mobile-native.mjs setup
node scripts/mobile-native.mjs build ios
node mobile/ios/scripts/build-rust.mjs --plan
node mobile/ios/scripts/build-rust.mjs
cd mobile/ios
pod install
cd ../..
xcodebuild -workspace mobile/ios/ShellfoxMobile.xcworkspace \
  -scheme ShellfoxMobile -sdk iphonesimulator -configuration Debug \
  -destination 'generic/platform=iOS Simulator' \
  -derivedDataPath tmp/ios-derived CODE_SIGNING_ALLOWED=NO build
```

Rebuild the canonical Rust archives after header changes; an older archive without the `_ex` exports is not a compatible runtime for this source. The Rust recipe uses target-specific, verified Ghostty source installations with a 15.1 deployment triple. Cargo uses `--locked`. It obtains native linker libraries/frameworks from actual target `rustc --print native-static-libs` output and saves those logs. Unknown linker options stop the build for review. It creates `tmp/mobile-rust/ios/ShellfoxMobileRust.xcframework`, a runtime podspec and project/Ghostty notices. Do not distribute an experiment artifact without the migration's Rust dependency/license audit.

The script refuses to overwrite an existing XCFramework. Remove only the old owned `tmp/mobile-rust/ios` output explicitly before rebuilding. Target preparation/Cargo caches live under `tmp/ios-rust-build`; they are build outputs, not credentials. No command above signs, installs or releases an application.

To include the native XCTest contracts, run `SHELLFOX_IOS_NATIVE_TESTS=1 pod install` from `mobile/ios`. Use `xcodebuild -list` on the generated workspace/Pods project to inspect the actual generated test scheme. Build its tests unsigned. Running them on a simulator/device requires a separately authorized test environment. The test spec checks real owner-thread ABI calls, immutable copied-frame lifetime, mailbox replacement, strict decimal identifiers, press/resize/release BUSY ordering without duplicate acceptance, reserved release capacity, bytes/hint preservation, mixed-batch rollback, internal-event filtering and marked-text commit/cancellation. `SFInputPipelineTests.mm` drives Ctrl+Space through the real owner encoder, client store, Rust client and production Go transport copy/worker/completion path. Only the final Go request function is replaced. It checks one NUL byte reaches the JSON payload as escaped data with `userInitiated:true`, then checks a Ghostty-encoded passive hover reaches that boundary with explicit false. It performs no network requests or authentication.

The standalone header contract can also be syntax-checked from an x64 MSVC developer prompt with `cl /nologo /std:c++20 /EHsc /Zs mobile\\ios\\tests\\abi-contract.cpp`, or with `clang++ -std=c++20 -fsyntax-only mobile/ios/tests/abi-contract.cpp` on a 64-bit Unix/macOS host. It uses the actual C headers, not substitute SDK or generated RN headers.

## Native v1 simulator smoke

The Apple platforms workflow builds a separate simulator bundle with the regular app code. The runner launches it with `--shellfox-control-url` against a loopback Tailscale test control plane and a Rust CLI host. The test approves the simulator through the host CLI and checks shell input, output and PTY size before and after iPad rotation. It stops and restarts the real host network listener, proves the attached session reconnects to the same shell, then backgrounds and foregrounds the app. It also checks that a file edit made in the app changed the host file on disk. The run uploads the XCTest result and screenshots in `ios-simulator-results`. It exercises the default terminal without using a real tailnet or signing identity.

## Verification status

Verified here on Windows:

- Installed RN schema/code generation and eight iOS contract/build-recipe checks through `node --test mobile/ios/scripts/contracts.test.mjs`.
- `pnpm typecheck`.
- Latest `pnpm test:unit mobile/src/rust-client`, 28 tests across six files.
- MSVC x64 `/std:c++20 /Zs` compilation of `mobile/ios/tests/abi-contract.cpp`, including real header layouts, additive `_ex` callback/write signatures and reset-grid/recovery/disconnect signatures. This does not compile Objective-C++ or UIKit.
- Final `pnpm test:native`, 171 C# tests and nine Node helper/pipe tests. These are regression checks, not iOS tests.

An initial `pnpm test:unit -- ...` invocation ran the broader suite rather than filtering it and timed out after 90 seconds. It is not a full-suite pass. The correctly filtered invocation passed.

The shared event schema now includes the current reset geometry/readiness fields, covered by the latest tests. Native readiness still needs wiring into JS special-key controls. Absolute follow-output-on-input behavior and legacy non-UTF-8 mouse-input transport were raised with the shared owners; this adapter does not add iOS-only escape encoders or magic scroll offsets to hide those gaps.

Blocked here: CocoaPods evaluation, Swift/Objective-C++ compilation, iOS Rust/Ghostty linking, unsigned simulator application build, XCTest execution, real IME/candidate keyboards, international hardware layouts, VoiceOver, touch selection, screenshots, sustained-output measurements and suspend/reconnect behavior. The build script confirms this with a Windows platform error. No native compile or runtime pass is claimed.

Use isolated fixtures for further device checks: full-screen TUIs, takeover/readonly, detach/reattach, lost replay, split UTF-8/ANSI, theme changes, keyboard/safe-area resizing, CJK/dead keys/AltGr/emoji, paste policy, native selection/copy/links, VoiceOver, background/foreground reconnect and slow-consumer pressure.
