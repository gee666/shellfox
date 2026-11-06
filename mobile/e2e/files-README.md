# Android Files demo and QA

These entries use the production Files screen with deterministic in-memory host data. They do not enroll Tailscale, connect to a peer, or change the installed normal mobile client's data.

## Demo entry

`files-entry.js` opens the actual directory browser immediately. Closing a file returns to its origin directory. Back at the root opens a compact Demo workspace row with a folder icon labelled `Browse files`. There are no fixture-launcher buttons, clipboard probes or pending-download controls in this entry.

`FilesHarness.tsx` is the demo shell. `files-fixture.ts` supplies its example code, Markdown, HTML, SVG and images. Native downloads and clipboard actions still use the real device bridges.

## Explicit QA entry

`files-qa-entry.js` imports `FilesTestLauncher.tsx`. Only this bundle exposes `Files QA launcher` and the internal fixture controls. Its `shellfox-files-qa://disconnect` and `://reconnect` commands are explicit component VIEW intents used to test offline close. No manifest or normal-client deep-link changes are required.

The QA entry also enables delayed-read and transfer-only fixtures. Native pickers, clipboard, WebViews and Downloads I/O are real; host responses are fixtures. It does not test authenticated peer transport. iOS native execution requires macOS and Xcode.

## Build both isolated APKs

Wait for mobile UI/native source changes to settle first. One owner should build both entries, rather than running concurrent Gradle builds. From the repository root, set `ANDROID_HOME` and `JAVA_HOME` to the Android SDK and JDK 17. The existing Android native AAR must already be built.

```powershell
node mobile/e2e/build-files-android.mjs both
```

This assembles only the debug-signed `terminalTest` variant and archives:

- `tmp/mobile-e2e/files-qa.apk`, using `ENTRY_FILE=e2e/files-qa-entry.js`
- `tmp/mobile-e2e/files-demo.apk`, using the default `ENTRY_FILE=e2e/files-entry.js`

Build logs are beside those APKs. Use `qa` or `demo` instead of `both` for a targeted rebuild. `SHELLFOX_ANDROID_ARCHS` defaults to `x86_64` for the emulator. The normal package `com.shellfox.mobile` is never installed, stopped, cleared or reconfigured by these runners.

## Real Windows remote-device proof

`files-remote-entry.js` is a separate, explicit QA bundle. It mounts the production `FilesScreen` and unchanged production `RemoteClient` against a real Windows `RemoteHost`, approvals, HMAC verification, chunk API and filesystem. Windows files are created under `tmp/mobile-real-remote-host/<run>/Windows-host-files`, not an Android in-memory map. The driver verifies native text/image saves on that disk and compares upload/download SHA256 outside Android.

```powershell
node mobile/e2e/build-files-android.mjs qa
node mobile/e2e/run-caret-android.mjs
node mobile/e2e/build-files-android.mjs remote
$env:SHELLFOX_FILES_KEEP_QA = '1'
node mobile/e2e/run-real-remote-files-android.mjs
Remove-Item Env:SHELLFOX_FILES_KEEP_QA
node mobile/e2e/build-files-android.mjs demo
node mobile/e2e/run-files-demo-android.mjs
```

The real-remote entry accepts only `http://10.0.2.2:41973`, its fixed synthetic QA peer, and production remote routes. Its temporary Gradle init script rejects other entries and tasks, adding a `terminalTest`-only network-security overlay whose base policy denies cleartext and whose sole exception is `10.0.2.2`. No production Gradle, native bridge or manifest policy is changed. APK inspection checks the overlay is absent from QA/demo builds and that demo Hermes contains none of the launcher control strings. The default mobile entry and release app do not import this transport.

The runner starts and stops only its own isolated Windows host. The host-runner control token remains in its Windows ready file and is never sent to Android. HMAC helper credentials are synthetic QA values, never an enrolled user's credentials. This tests Android-to-Windows device I/O through a QA loopback relay. **It does not test TSNet enrollment or tailnet dialing.** Those production transport layers are not replaced or loosened in release builds.

Successful real-device artifacts include `real-device-proof.json`, Windows-side request/file hashes, and native PNGs showing the actual Windows path, edited file, assets, cropped image, picker upload and MediaStore download. The completed verification logs are `tmp/mobile-caret-atomic-native.log` and `tmp/mobile-real-remote-android.log`. The native caret gate proves index-two placement with a pre-toggle insertion, then Read/Edit insertion without Tab, pointer repositioning or Ctrl+A, and verifies saved text.

## Run native regressions

Boot an isolated emulator and leave its UI untouched while a runner executes.

```powershell
$env:ANDROID_SERIAL = 'emulator-5554'
node --test mobile/e2e/files-layout.test.mjs
node mobile/e2e/run-files-android.mjs
node mobile/e2e/run-file-transfers-android.mjs
```

The QA entry keeps the initial plain-text clipboard only in a JS ref. It restores that text before QA restarts and demo restoration, but only if the clipboard still contains the fixture's own copy. It never logs or renders the original value. Non-text clipboard formats cannot be restored through the Android library, so copy/keyboard screenshot QA refuses to replace them rather than destroying them. The clean demo does not import this helper.

Both runners require the QA and demo archives. The first optional argument overrides the QA APK; `SHELLFOX_FILES_DEMO_APK` overrides the demo archive. They refuse physical-device serials and packages other than `com.shellfox.mobile.terminaltest`. They assert the QA launcher marker before using internal controls and restore/install the clean demo entry when finished, including after a test failure. The final visible screen is the real Files browser, not the QA launcher.

The Files runner covers dirty close, native edit/save, wrapped code, path sheets and menus, pending-read cancellation, offline origin-directory snapshots, interactive HTML and sibling assets, Markdown/SVG/ICO previews, and image rotate/touch-crop/save. It measures actual native bounds at 320dp, 360dp and landscape: one toolbar row, at least 80% safe-area content, full-width folder/file touch targets, centered upload/download glyphs, actual EditText/Go alignment and all nine image actions. The landscape path test requires the full software keyboard, not merely the hardware-keyboard toolbar. On the current AVD, Gboard exposes its floating stylus widget instead of bottom QWERTY even when requested. The post-fix no-extract flags and visible app X/input/Go were verified, but full-QWERTY reachability is an explicit device-mode limitation, not a passed check. `SHELLFOX_FILES_SKIP_PATH=1` is only for isolated layout captures and must not be reported as keyboard coverage. Display overrides and the hardware-keyboard IME preference are restored afterward.

The transfer runner uses Android's system document picker and MediaStore Downloads. It verifies multi-chunk and empty files, exact hashes, files above the preview cap, no-clobber uploads, cancellation and abandoned pending-download recovery. Cleanup is restricted to test-package-owned `shellfox-transfer-*` MediaStore rows; unexpected remaining destination-name collisions fail rather than overwrite another file.

Artifacts go under `tmp/mobile-files-native/<timestamp>/` and `tmp/mobile-file-transfers/<timestamp>/`. Files captures include native hierarchy JSON, `layouts.json` measurements and a `captures.json` contact-sheet manifest. Only explicit QA builds emit read-only viewport/status diagnostics to app logs; the demo does not render or import those tools.

Compose a review sheet from actual native screenshots without altering app pixels:

```powershell
node mobile/e2e/compose-files-preview.mjs tmp/mobile-files-native/<timestamp>/captures.json tmp/mobile-ui-redesign-preview.png
```

For isolated review, `SHELLFOX_FILES_GEOMETRY_ONLY=1` runs native layout captures first, and `SHELLFOX_FILES_GEOMETRY_WIDTHS=640` limits that pass to landscape. `SHELLFOX_FILES_FUNCTIONAL_ONLY=1` runs edit/save/navigation/WebView regressions without repeating the approved geometry matrix. `SHELLFOX_FILES_IMAGE_ONLY=1` selects the shorter image path.

`SHELLFOX_FILES_KEEP_QA=1` is an explicit reviewer-only option. It restores the clipboard but leaves QA installed until PNG review. After signoff, run `node mobile/e2e/run-files-demo-android.mjs` to install the default demo, verify normal folder/workspace navigation, and leave the actual browser interactive. Do not hand the QA launcher to a user as the demo landing screen.

Keep the emulator untouched during automation. Native input waits for stable target bounds, but manual touches or an IME overlay can still interfere.
