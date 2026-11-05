# Android Files integration test

`FilesHarness.tsx` mounts the production Files screen with an in-memory, revision-checked FileApi. It checks the real React Native controls and Android WebViews without enrolling Tailscale, opening a host session, or changing production app data. Backend filesystem and authenticated peer transport tests run separately.

The runner refuses physical-device serials and APKs outside `com.shellfox.mobile.terminaltest`. It tests folder navigation with session/tab scope, syntax-highlighted code, native text editing and saving, dirty confirmation, Markdown sibling images, interactive HTML with sibling JavaScript, SVG/ICO previews, canvas image rotation and saving, and Android back navigation.

## Build and run on Windows

Run from the repository root with dependencies and the existing native Android AAR built. Set `ANDROID_HOME` and `JAVA_HOME` to the Android SDK and JDK 17. Boot an emulator first. This uses the existing isolated `terminalTest` Gradle variant and its debug test signer, never release credentials.

```powershell
$root = (Get-Location).Path
$env:TMP = "$root/tmp"
$env:TEMP = "$root/tmp"
$env:TMPDIR = "$root/tmp"
$env:GRADLE_USER_HOME = "$root/tmp/gradle-home"
$env:ENTRY_FILE = 'e2e/files-entry.js'
Push-Location mobile/android
try {
  & "$env:JAVA_HOME/bin/java.exe" -classpath gradle/wrapper/gradle-wrapper.jar `
    org.gradle.wrapper.GradleWrapperMain --no-daemon --console=plain `
    -PreactNativeArchitectures=x86_64 :app:assembleTerminalTest
  if ($LASTEXITCODE -ne 0) { throw 'Fixture APK build failed' }
} finally {
  Pop-Location
  Remove-Item Env:ENTRY_FILE
}
$env:ANDROID_SERIAL = 'emulator-5554'
node mobile/e2e/run-files-android.mjs
```

The runner accepts an optional APK path. Artifacts and app-only logs go under `tmp/mobile-files-native/<timestamp>/`. Set `SHELLFOX_FILES_IMAGE_ONLY=1` for the short image-editor regression path. Image cropping uses a real touch drag, then the compact WebView Crop and Save controls. No numeric crop fields are used.

For real native file transfers, run `node mobile/e2e/run-file-transfers-android.mjs` with the same optional APK path. This uses the Android system document picker, the native bounded chunk reader, and MediaStore Downloads. The remote API is a deterministic fixture. It verifies multi-chunk and empty files, exact download hashes, a file larger than the preview cap, create-without-overwrite, cancellation, pending-download recovery after process death, and hardware Escape. Only test-owned `shellfox-transfer-*` Downloads files are touched. Artifacts go under `tmp/mobile-file-transfers/<timestamp>/`.

Keep the emulator untouched while a runner executes. Its UIAutomator coordinates are device input, so manual touches or a floating IME over a control can interfere with the test.

`compose-files-preview.mjs` builds a 2x2 contact sheet from native screenshot paths in `tmp/mobile-file-transfers/preview-captures.json`, written by the capture run. It does not generate or alter application artwork.

The fixture APK is not a production mobile build and does not test a real peer connection. iOS device testing requires macOS and Xcode.
