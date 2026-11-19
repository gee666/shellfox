# Native Rust terminal on Android

Android uses the Rust V1 client and Ghostty Fabric terminal by default, with the same Sessions and Files screens. It uses official libghostty-vt 1.0.1 and works with V1 hosts. The `shellfoxManagerV2` build flag and launch profile still select the separate Native002 experiment. iOS keeps its WebView default pending its simulator run.

## Ownership and data path

`ShellfoxRustPackage` registers `NativeShellfoxRustClient` and `ShellfoxRustTerminal`. The module extends the generated `NativeShellfoxRustClientSpec`. The view manager implements the generated interface and uses its delegate. Codegen comes from `ShellfoxRustSpecs`, with Java package `com.shellfox.mobile`, in `mobile/package.json`. Do not hand-edit generated classes.

`TerminalCoordinator` owns one process-lifetime `HandlerThread`. Module creation, rendering, configuration, parser barriers, resize, selection, input, reset and close all use that OS thread. A mounted view borrows a module-owned engine. Unmounting does not kill a host terminal or close an engine behind the module's back. One engine can have only one mounted sink. Handle strings and progress sequences preserve all 64 bits.

`ClientTerminalBindings` binds engines before attach. Rust validates host epochs and cursors, splits delivery into bounded engine tickets and grants ACK credit only after parsing. Native calls `terminal_consumed` again when credit admission is busy, even if no new frame is needed. Pending output is parsed before mode-dependent input encoding or resize. An accepted-ticket barrier cannot move when more output arrives.

Recovery events stay native. The owner queries the current ticket and `terminal_reset_grid` for that exact ticket, resets at the live host grid, then completes the matching ticket. A stale query is retried. Queued event dimensions are never used. A fence-only reset keeps the existing native grid. Engines pump even before any view mounts. Native waits for `terminal_recovery_ready`, then fits the recorded local viewport and exposes the coarse attachment result. Read-only views never request host resize. Retired IME connections cannot commit into a new view binding or reset epoch.

Rust owns the accepted write/resize FIFO. Production encoding uses `terminal_input_ex`, then passes its Rust-computed 0/1 hint unchanged to `client_terminal_write_ex`. The hint and explicit-length bytes stay together through every retry, including embedded NUL. Platforms and JS do not classify the wire hint. A bounded native pre-admission buffer retains only BUSY-rejected intents, with reserved release capacity. A rejected release cannot be overtaken by later input or resize. It never queues or reorders accepted Go requests. Reset/unbind fences discard old-generation unaccepted intent. Layout changes can coalesce only when no input separates them.

`NativeGoTransport` copies borrowed peer/path/body bytes during dispatch. Its separate zero-queue worker pools call the existing outbound-only `Mobile.request`. Rust retains no JNI callback context. Native deadlines and cancellation fence late results. Go Request cannot currently abort one HTTP request, so its worker can remain occupied until Go returns. Both the existing network module and this adapter use the same bounded pair-approval wake-lock helper for screen-off parity. There is no host secret, local PTY, alternate HTTP client or automatic login/pairing here.

Explicit JS operations receive coarse completion events. Native ACK/write/resize successes are filtered; repeated internal errors are coalesced. Explicit polls retain their completion so the existing JS operation scheduler can continue. Output chunks and cell/frame data never enter JS. There is no second native auto-pair or auto-poll loop.

Close requests graceful remote disconnect, continues dispatch/completion for up to three seconds, then fences and closes the native client. It is best effort when offline. Invalidation waits for that bounded teardown before engine cleanup. Host terminals survive client closure.

## Drawing and interaction

Rust's borrowed frame is copied before its callback returns. A bounded immutable front/pending mailbox replaces stale pending frames rather than queuing UI work. Android Canvas shapes style runs with full-row context and system monospace font fallback. The renderer handles cell colors, inverse/faint/invisible text, decorations, wide-cell spacers, cursor and native selection. Font size uses Android SP scaling; layout uses physical pixel cell dimensions. Theme JSON first passes Rust validation and then configures the native cursor, selection and uncovered background. The public ABI currently permits a steady block cursor only.

`TerminalInputConnection` keeps UTF-16 marked text separate from committed text. It commits once, handles codepoint-aware deletion, keeps partial UTF-16 preedit out of committed UTF-8, and rejects edits from retired connections. Cursor-anchor updates expose marked-text selection and insertion geometry to the IME without exposing the terminal transcript. Hardware input preserves press/repeat/release and consumed AltGr modifiers. Focus loss releases held keys. Reset discards old-generation held keys. IME newline/tab and accessory keys use typed Ghostty events, not hand-built escapes. Sticky modifiers are a coarse prop.

Touch scroll uses native scrollback, or Ghostty mouse encoding when mouse tracking is active. Long press and selection drag use Rust selection APIs. Tapping a file path or an OSC 8 file link opens it on the current host through Files. The long-press menu opens OSC 8 web links only over HTTP/HTTPS and rejects embedded credentials.

Accessibility exposes terminal text and editing/copy/scroll actions. This is basic single-node access, not validated screen-reader parity. Background views keep bounded parsing/credit alive without copying frames; the RN host-client scheduler owns suspension and generation-fenced reconnect.

## Build inputs

The existing app and RN 0.88 effective minimum are API 24. Do not raise them to API 26 to satisfy a build recipe. Checked tools are JDK 17, NDK `27.1.12297006`, CMake `3.22.1` and wrapper Gradle `9.4.1`.

The existing Go build remains:

```sh
SHELLFOX_GO="<absolute project-local Go 1.27.3 executable>" node scripts/mobile-native.mjs setup
SHELLFOX_GO="<absolute project-local Go 1.27.3 executable>" node scripts/mobile-native.mjs build android
```

It produces `tmp/mobile-native/android/shellfox-mobile.aar` and notices. The canonical script appends `-Wl,-z,max-page-size=16384` to Android `CGO_LDFLAGS`, preserving caller flags. It never adds that ELF option to iOS Mach-O builds. A rebuild with the caller's CGO flags unset passed Android's 16 KiB check, and NDK readelf reported `0x4000` alignment for both packaged ABIs. Eight Node build-plan regressions pass. No global Go tools or Android app installation is required.

Before running Gradle, set `ANDROID_NDK_HOME` to the pinned NDK and build the API-24 Rust libraries with the pinned Zig 0.17.0. Install the Rust Android targets and fetch Cargo dependencies first.

```sh
node crates/shellfox-mobile/build-mobile.mjs build-android --zig /path/to/zig
```

Gradle reads the resulting build directory under `tmp/mobile-rust-produced/android`. Use `-PshellfoxRustAndroidDir=<directory>` for an isolated build. The separate JNI library links with undefined symbols forbidden and 16 KiB page alignment. The APK ships `arm64-v8a` and `x86_64`.

## Verification

To run the real app against a test tailnet, build `:app:assembleTerminalTest` with the normal `mobile/index.js` entry. Install `com.shellfox.mobile.terminaltest` only on the test emulator. Use `adb reverse` for the loopback control and DERP ports, and pass `--es shellfoxControlURL http://127.0.0.1:<control-port>` when starting `com.shellfox.mobile.terminaltest/com.shellfox.mobile.MainActivity`. The Go library validates the loopback origin and uses separate state. The normal V1 app build does not accept this launch extra.

Run from the repository root. Keep `GRADLE_USER_HOME` under project `tmp/` when invoking the wrapper. Each shell invocation needs an outer timeout.

```sh
mobile/android/gradlew -p mobile/android --no-daemon \
  :app:generateCodegenArtifactsFromSchema :app:compileDebugKotlin \
  :app:testDebugUnitTest :app:compileDebugAndroidTestKotlin :app:lintDebug
```

On Windows use `gradlew.bat`. Unit coverage includes the frame codec/mailbox, unsigned IDs, real Android API-24 IME/hardware normalization under Robolectric, explicit poll completion, and internal-event suppression. Robolectric is not hardware IME acceptance.

For an NDK-only ABI compile check without claiming a native link:

```sh
<SDK>/cmake/3.22.1/bin/cmake -S mobile/android/app/src/main/cpp \
  -B tmp/android-jni-check/arm64 -G Ninja \
  -DCMAKE_TOOLCHAIN_FILE=<SDK>/ndk/27.1.12297006/build/cmake/android.toolchain.cmake \
  -DANDROID_ABI=arm64-v8a -DANDROID_PLATFORM=android-24 \
  -DSHELLFOX_JNI_COMPILE_ONLY=ON
<SDK>/cmake/3.22.1/bin/cmake --build tmp/android-jni-check/arm64
```

Repeat for `x86_64`. `src/test/cpp/terminal_frame_test.cpp` is a host-executable fixture for copied callback data, packed fields and invalid frame rejection. The six `NativeTerminalFixtureTest` instrumentation cases compile against the real JNI API and need linked native libraries plus a separately authorized isolated device/emulator installation to execute. The end-to-end wire case uses the actual Rust encoder/client and JNI dispatch with test-only protocol responses to verify NUL and passive-input metadata. They do not start tsnet, use an account, or spawn a PTY.

### V1 snapshot evidence

- RN 0.88 Gradle codegen, Kotlin/Java compilation and instrumentation-source compilation passed with the real Go AAR.
- Full standalone JNI shared-library links passed at API 24 for arm64-v8a, x86_64, armeabi-v7a and x86, with warnings treated as errors and undefined symbols forbidden. The configured app/Go packaging ABI set remains arm64-v8a and x86_64; a standalone link does not add packaged 32-bit support.
- The host C++ frame-copy fixture passed with MSVC `/W4 /WX`.
- Thirty JVM tests passed: five frame/mailbox/ID tests, twelve API-24 IME/hardware tests, four event/live-grid tests, five pre-admission/metadata tests, two pair-approval wake-lock tests and two API-24 default-route tests. No skips.
- Android `lintDebug` passed with zero errors and 33 existing-resource/template warnings. API-26-only unsigned Long helpers were replaced by API-1 BigInteger conversion/sign-bit comparison. The shared Android network provider's API-29 gateway call was replaced with an API-24-compatible predicate. No minimum SDK increase or suppression was used.
- A scoped run passed 51 existing mobile/adapter Vitest tests in seven suites. `pnpm test:native` passed 79 .NET tests and five pipe-run tests, with no skips. These do not establish Android device behavior.
- Final root `pnpm typecheck` passed, including mobile. Earlier concurrent runs encountered unfinished TS edits and visual-reference import errors outside Android; those were resolved by their owners before the final run.
- Actual `assembleNativeFixture` and `assembleNativeFixtureAndroidTest` passed, including the generated Fabric `appmodules` and JNI image links for both packaged ABIs. Packaged appmodules/JNI/Go libraries and all four standalone JNI libraries have `0x4000` PT_LOAD alignment.
- Under explicit approval, six JNI/Ghostty instrumentation tests passed on a fresh project-owned AOSP-35 x86_64 emulator, including NUL and passive-input hints through the actual Rust client wire. The plain fixture Application never starts React Native or tsnet and has no launcher or network permission. No production package, physical device, account or real pairing was used.
- The reported stale required-grid field on reconnect was cleared upstream. Actual native reconnect/recovery still needs runtime acceptance against the final canonical archives.

### Approved isolated runtime check

The tested archives came from compiled snapshot `91ed48bc846230f43b09cf12f3e0931825c269b86c6f98c73a5997951bb04964`, not an assertion about HEAD. All four per-ABI archive/manifest hashes, snapshot-origin digests, Cargo lockfile and live/staged header hashes were verified before installation. Evidence is retained under `tmp/android-native-fixture/evidence/`, especially `verified-archives.json`, `native-results.json` and `instrumentation.txt`.

Build the isolated package without changing normal app defaults:

```sh
mobile/android/gradlew -p mobile/android --no-daemon --max-workers=2 \
  -PshellfoxAndroidTestBuildType=nativeFixture \
  -PreactNativeArchitectures=arm64-v8a,x86_64 \
  :app:assembleNativeFixture :app:assembleNativeFixtureAndroidTest
```

It uses `com.shellfox.mobile.nativefixture` and a separate test package. Never use an automatically discovered connected device for this check. The approved run used only `emulator-5584`, private ADB port `31537`, project-local AVD/user/auth directories, two vCPUs, 2048 MiB RAM, existing WHPX acceleration and no snapshots. Both processes were stopped gracefully; all owned ports and process exit were verified before deleting disposable userdata and keys.

The emulator reported API 35 and `PAGE_SIZE=4096`. This is neither API-24 runtime nor 16-KiB-page runtime acceptance. The React Native screen, real IMEs, touch rendering and screen readers were not exercised by these backend fixtures.

Disposable scratch can be removed after checks. The project-local `tmp/android-rust-check/gradle-home` contains retained Gradle 9.4.1/RN build prerequisites for the final archive/link check, and `tmp/android-rust-tests/maven` retains the API-24 Robolectric dependency. Go tools, AAR and notices are retained under the existing `tmp/mobile-native` build path.

Still required: final packaged dependency/notice and legal review, actual CJK/AltGr/dead-key/emoji keyboards, mixed-direction text and font fallback/DPI screenshots, SGR pixel-mouse coordinate fidelity with fractional font metrics, touch/scrollback/selection and TalkBack checks, slow-consumer/recovery and background lifecycle tests, host mutation-order tests, representative TUI use, and release performance measurements. None of these is accepted merely because source or codegen compiles.
