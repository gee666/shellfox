# Mobile builds and updates

The React Native app is under `mobile/`. Android builds produce a universal APK containing arm64-v8a and x86_64 libraries. iOS uses the same client code but requires macOS, Xcode, CocoaPods and an iOS signing/distribution setup. An APK is not an iOS distribution.

Sessions use compact title/path rows and the shared desktop status-dot logic. Search opens from the header icon. Settings includes the same accent/background presets, custom-color history, picker, contrast calculation, and terminal palette as the desktop app. Colors are local to the mobile client and never change host settings.

The terminal row has separate system-keyboard and special-key icons. They toggle independently but never remain open together. The special panel provides modifiers, function keys, navigation and editing keys through xterm's encoder. The same accessory row presents touch directory suggestions above the keyboard in the session dialog. OK resolves the path first; a missing directory prompts for recursive creation. No returns to the unchanged draft. Tap the tab plus to use the default shell, or hold it to choose a profile.

Both clients show live directory matches. Desktop arrows choose a candidate, the first Enter accepts it, and the next submits the chosen directory. Tab completion remains available. A sole exact match is hidden; ambiguous siblings stay visible. Mobile suggestions select the path without creating a session.

## Local Android build

From the repository root:

```sh
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test:unit mobile/src
pnpm test:packaging
node scripts/mobile-native.mjs test
node scripts/mobile-build.mjs --test
```

`--test` builds the `preview` variant. It embeds the JavaScript bundle and terminal HTML, disables developer support, and signs with the scaffold's debug keystore. It needs no Metro server. The APK is written to `tmp/mobile-artifacts/shellfox-mobile-<version>-android-test.apk`, with a `.sha256` sidecar. It is never uploaded as the stable APK. `--test` is rejected on GitHub tag refs.

The build script generates terminal HTML, sets up gomobile when absent, builds the native AAR, invokes the pinned Gradle wrapper, then checks the actual APK with `apksigner` and `aapt`. It verifies the package name, version, signer, non-debuggable status and both ABIs before copying the artifact. A failed build removes its publishable output.

Prerequisites:

- Node 24.15.0 and pnpm 12.9.1, matching the desktop release.
- JDK 17 with `JAVA_HOME` set.
- `ANDROID_HOME` pointing to an Android SDK with platform 36, build-tools 36.0.0, NDK 27.1.12297006 and CMake 3.22.1.
- Go 1.27.3 and the pinned gomobile toolchain. `scripts/mobile-native.mjs` uses `SHELLFOX_GO`, the repository's `tmp/go-toolchain/go/bin/go` cache, or Go on PATH. It supports `setup`, `test` and `build android` commands.
- Network access for the first Gradle, Maven, Go and Android package downloads.

Install missing Android packages using the SDK's `cmdline-tools/latest/bin/sdkmanager`:

```sh
sdkmanager 'platforms;android-36' 'build-tools;36.0.0' 'ndk;27.1.12297006' 'cmake;3.22.1'
```

Windows uses `sdkmanager.bat`. The build script invokes Java directly rather than relying on platform-specific batch quoting. Node, Java, Go and Android build tools must all be installed for the same host OS. Do not mix Windows and WSL toolchain paths.

## iOS development

On macOS with Xcode 16.1 or newer, an iOS SDK, Go, and CocoaPods installed:

```sh
pnpm install --frozen-lockfile
node scripts/mobile-terminal.mjs
node scripts/mobile-native.mjs setup
node scripts/mobile-native.mjs build ios
cd mobile/ios
pod install
open ShellfoxMobile.xcworkspace
```

Choose a development team in Xcode and run the app on a simulator or device. The local pod links the generated `Mobile.xcframework`; the same React Native screens and host protocol run on both platforms. iOS does not request a device-wide VPN entitlement or promise background terminal connectivity. On return to the foreground, it reconnects to the host-owned shells. APK installation is Android-only.

The Xcode app version currently matches root `0.9.2`, build `9002`. Until iOS distribution is automated, update both Xcode configurations when changing the root release version. These sources have not been compiled on this Windows development machine. A Mac build and iOS device acceptance are still required. iOS is intentionally absent from GitHub release builds.

## Emulator and E2E

The Windows development machine was inspected with these results:

| Tool | Available locally |
| --- | --- |
| Node / pnpm | 24.15.0 / 12.9.1 |
| Java | Microsoft OpenJDK 17.0.15 |
| Android SDK | `C:\Users\user\AppData\Local\Android\Sdk` |
| Platforms | 34, 35, 36 |
| Build-tools | 33.0.1, 34.0.0, 35.0.0, 36.0.0 |
| NDK | 21.4, 25.1, 26.3, 27.0, 27.1.12297006 |
| CMake | 3.18.1, 3.22.1 |
| Android Studio / ADB | Installed, ADB 36.0.0 |
| Emulator | Installed, WHPX acceleration reports usable |
| AVDs | `Medium_Phone_API_35`, `Medium_Phone_API_36.0` |
| Go | Repository-local toolchain under `tmp/go-toolchain`, not on PATH |
| iOS tooling | Xcode, Swift and CocoaPods not available on this Windows host |
| Mobile E2E tools | No Maestro, Detox or Appium CLI found on PATH during inspection |

The emulator and SDK command-line directories are not on PATH. Use their full paths or add them to your shell's PATH. Initial inspection found no connected device. Inspect again before running tests:

```sh
emulator -accel-check
emulator -list-avds
emulator -avd Medium_Phone_API_35 -no-snapshot
adb devices -l
adb wait-for-device
adb shell getprop sys.boot_completed
adb install -r tmp/mobile-artifacts/shellfox-mobile-0.8.0-android-test.apk
adb shell am start -n com.shellfox.mobile/.MainActivity
```

Replace the APK version with the root package version. Wait for `sys.boot_completed` to return `1`. Run emulator startup in a separate terminal. Desktop Playwright tests launch Electron and cannot validate a native Android APK. Mobile acceptance must exercise the installed APK, its WebView terminal, keyboard input, host approval, reconnection and app lifecycle. Use the repository's mobile E2E runner when available, not the Electron `test:e2e` command.

For updater acceptance, install an older signed build and publish a newer stable build signed by the same key. Check the download, unknown-source permission denial and approval, installer cancellation, and successful interactive upgrade. Verify that wrong checksums, package names, certificates and version codes cannot reach the installer. A debug-signed preview cannot upgrade to the production signing identity. Uninstall it before testing production builds, which also removes its app data.

## Terminal input and connection status

Connection state stays visible in the headers instead of leaving obsolete reconnect warnings. Closed tabs no longer try to attach to a nonexistent terminal. Temporary reconnects retain read-only session metadata with loading dots until a fresh snapshot arrives; explicit disconnect and logout clear it.

Native requests reuse identity-bound HTTP connections after checking current peer ownership. Idle sockets expire before the host's timeout. Input and rendered-output acknowledgments coalesce without changing byte order, replaying commands, or crossing resize boundaries. Pi requires two Ctrl+C interrupts within 500 ms to exit. The encoder handles multi-character IME input under Ctrl, and pooled requests remove repeated connection setup; an unusually high-latency link can still exceed Pi's own interval. The client never invents a second interrupt.

## Host discovery

`ShellfoxNetwork.hosts(): Promise<string>` returns bounded JSON with `{ peers, discovering }`. `discoverHosts()` in `mobile/src/network.ts` validates that response and returns `{ peers: NetworkPeer[], discovering: boolean }`. The compatibility wrapper `networkPeers()` returns only `peers`. These APIs list confirmed Shellfox hosts, not every Tailscale peer. An empty list with `discovering: true` is not a settled empty result.

Native discovery returns cached hosts immediately and probes in the background. Up to four probes run independently of terminal requests and event polls. Each candidate has one two-second budget, including any legacy fallback, and a 4 KiB response limit. Probes use tsnet, same-owner untagged peers, and TCP 52743 only. Concurrent snapshots share a scan. Positive results are cached for five minutes and negative results for 30 seconds, so 250 ms progress polling or three-second settled polling does not repeat network probes. Confirmed hosts remain listed when offline or temporarily unreachable, provided they remain in the current same-owner peer map.

New hosts answer authenticated `POST /v1/discover {}` with `{ ok: true, value: { service: 'shellfox', protocolVersion: 1 } }`. This bypasses pairing and approval without exposing sessions or creating a pending request. A 404 permits one compatibility probe, `POST /v1/request {}`. Only the exact legacy Shellfox `VALIDATION` envelope with message `Remote request could not be completed.` and `retryable: true` confirms a legacy host. Empty request validation happens before approval or client allocation. Discovery never sends a client ID, requests pairing, or uses hostname prefixes.

Only confirmed IDs are persisted in the private atomic node state store, bound to the local node and signed-in owner. Successful logout clears them. Ordinary peers and UI remembered IDs cannot populate this cache. After restart, confirmed remembered hosts can reconnect on the first snapshot without waiting for unrelated probes. Corrupt or differently owned discovery records are ignored.

## Local host for a real mobile connection

The phone is a client, not a local PTY host. Build a headless host on the development OS and keep it running in a separate terminal. These commands use a disposable data directory under project `tmp/`, not the normal desktop database.

Windows PowerShell:

```powershell
pnpm build:headless
$cli = "$PWD\tmp\headless\shellfox-headless-win32-x64\shellfox.ps1"
$data = "$PWD\tmp\mobile-host"
& $cli --user-data $data host run --json
```

In another PowerShell terminal at the repository root:

```powershell
$cli = "$PWD\tmp\headless\shellfox-headless-win32-x64\shellfox.ps1"
$data = "$PWD\tmp\mobile-host"
& $cli --user-data $data network enable --hostname shellfox-mobile-test --json
& $cli --user-data $data network login --json
& $cli --user-data $data session create "$PWD" --title Mobile --json
& $cli --user-data $data approvals list --json
```

Open the returned login URL and sign the host and mobile app into the same Tailscale account. Connect from the app, list approvals again, then approve the mobile device's actual ID:

```powershell
& $cli --user-data $data approvals approve DEVICE_ID --json
& $cli --user-data $data host status --json
# After testing, this ends test-host terminals too.
& $cli --user-data $data host stop --yes --json
```

On Linux x64, use `./tmp/headless/shellfox-headless-linux-x64/shellfox`; on macOS ARM64, use `./tmp/headless/shellfox-headless-darwin-arm64/shellfox`. Substitute the native architecture where needed and pass an absolute `--user-data` directory consistently. The same host/network/session/approval commands apply. Source host builds also need the desktop/headless native compiler prerequisites documented in [headless.md](headless.md).

A real connection requires authenticated devices in the same supported tailnet, host approval, and tailnet rules allowing TCP 52743. An emulator's internet connection alone is not proof of pairing. These commands do not claim that authenticated two-device access, startup after reboot or terminal survival across host restarts has been verified.

## Stable GitHub release

`.github/workflows/release.yml` retains the existing desktop and headless matrices and adds a signed Android job. There are seven product/OS combinations, not seven files:

- Desktop Windows x64, Linux x64/arm64 and macOS x64/arm64. Windows ARM uses the released x64 desktop through emulation.
- Headless Windows, Linux and macOS, each x64/arm64.
- One universal Android APK with arm64-v8a and x86_64.

Set these GitHub Actions secrets before pushing a release tag. No release has been created or published as part of this work:

| Secret | Value |
| --- | --- |
| `SHELLFOX_ANDROID_KEYSTORE_BASE64` | Base64 of the production keystore, without whitespace |
| `SHELLFOX_ANDROID_STORE_PASSWORD` | Keystore password |
| `SHELLFOX_ANDROID_KEY_ALIAS` | Production signing alias, never `androiddebugkey` |
| `SHELLFOX_ANDROID_KEY_PASSWORD` | Private-key password |
| `SHELLFOX_ANDROID_CERT_SHA256` | SHA-256 of the DER signing certificate, 64 hex characters without colons |

Keep an offline backup of the production key. Updating an installed APK requires the same signing certificate. This updater deliberately requires an exact signer set and does not implement signing-key rotation.

The script decodes the keystore under project `tmp/`, verifies its certificate against the configured fingerprint, rejects the scaffold debug certificate, and passes signing values as Gradle environment properties. Passwords are not command-line arguments. It verifies the signed APK again and removes the temporary keystore on completion or failure. Missing signing secrets stop the job. There is no release debug-key fallback. Direct Gradle release builds also require all four `SHELLFOX_UPLOAD_*` properties and reject the known debug alias/path.

The root `package.json` is the version authority for Android, desktop and headless. Android's Gradle configuration reads that file directly; `mobile/package.json` is not an independent native release version. Use stable `X.Y.Z` components between 0 and 999. Android `versionCode` is `major * 1000000 + minor * 1000 + patch` and must be positive. It increases with each stable version. Push a matching `vX.Y.Z` tag. The mobile build rejects mismatches and prerelease tags.

The mobile artifact is `shellfox-mobile-<version>-android.apk`, plus its `.sha256` sidecar. The existing release job downloads `shellfox-*` artifacts, waits for desktop, headless and mobile success, and publishes all assets under the same GitHub Release with generated notes. A manual workflow run builds but does not publish. Stable publishing requires signing secrets even for manual builds.

Desktop assets and their names remain unchanged. Windows still ships Squirrel `ShellfoxSetup.exe`, ZIP and NuGet files, Linux ships DEB and ZIP, and macOS ships ZIP. The existing desktop updater selects exact repository asset names, checks GitHub's size/digest metadata and uses its established platform installer handoff. Headless archives retain their `.sha256` sidecars. Android does not use the desktop updater or Squirrel.

## APK update contract

`mobile/src/updates/native.ts` exports `updates.check()` and `updates.install()` for the UI:

- `check()` returns a newer verified `AndroidUpdate`, or `null`. Network failures reject so the UI can offer retry. On iOS it returns `null` without fetching APK metadata.
- `install()` fetches fresh metadata again, so a stale notice never authorizes a cached asset. It returns `false` if no installable update is available, and rejects concurrent calls, network errors and native installation errors.
- Successful `install()` means Android's installer opened. It does not mean the user approved installation or the upgrade finished. The UI must not claim otherwise.

Metadata comes only from `https://api.github.com/repos/gee666/shellfox/releases/latest`. Validation rejects draft/prerelease metadata, non-stable tags, non-upgrades, duplicate matching APK assets, wrong repository/tag/name URLs, credentials/query/fragment/port additions, invalid sizes and missing SHA-256 digests. GitHub's release API must provide `digest: sha256:<hex>` for the APK. The sidecar helps manual verification but is not a substitute for missing API digest metadata.

The native bridge is `ShellfoxUpdates`, with `getVersion(): Promise<string>` and `install(url, sha256): Promise<void>`. Android validates the exact repository URL again, follows only HTTPS GitHub release/CDN redirects, downloads into private `cacheDir/updates`, bounds download size and time, and checks SHA-256. Before opening the installer it checks the APK package, version name, increasing version code and exact installed signing certificates. A release app rejects a debuggable candidate.

Android 8 and newer require permission to install from Shellfox. If absent, the module opens the per-app unknown-source Settings screen and waits for its activity result. Denial rejects the request. Approval rechecks the saved file and opens the normal system installer. No installer is launched silently, no permission is granted programmatically, and no root or shell installation path exists. A non-exported FileProvider grants temporary read access only to files under the dedicated update cache directory. APKs are retained for the installer, and old cached files expire on a later update attempt.

The iOS bridge returns its installed version and rejects `install()` with `UNSUPPORTED`. iOS updates must use its distribution channel, not APK downloads.

The trust model is the pinned GitHub repository plus the installed APK's signing key. Checksums alone are not proof of publisher identity. Native certificate comparison supplies the second check. Signing secrets must remain protected even if repository publishing access is compromised.

## Verification limits

TypeScript tests cover release validation, numeric version ordering, untrusted URLs, malformed assets, offline checks, timeout/abort, fresh installation checks, concurrency and iOS rejection. Packaging tests cover release dependency gates, required secrets, version codes, architecture coverage and separate preview signing.

Verified locally on Windows with the bundled Android preview APK:

- Root and mobile typechecks, including the isolated native fixture; the full unit suite passed 2,798 tests with 50 platform/prerequisite skips, using one worker on Windows.
- 65 .NET native tests, desktop Tailscale offline tests, mobile Go discovery and pooled-transport tests, three native build-script contracts, and 32 packaging/security checks.
- Fourteen bundled-terminal Chromium checks, including input, Unicode, repeated Ctrl shortcuts, multi-character IME commits, function/modifier keys, bracketed paste, replay, resize, touch selection, stale callbacks, live theme changes, and blocked resource loading.
- The isolated Android terminal fixture mounted the actual production WebView, rendered replay, accepted keyboard input, kept the key row above the IME, refit the terminal grid, opened and dismissed the keyboard from its button, and remounted without crashing. It also verified exclusive system/special keyboard toggling, exact F1/Ctrl+C bytes, native directory suggestions above the IME, path selection and caret position, and recursive-create Yes/No behavior. Its package is separate from the normal app, and its runner rejects any other APK package before installing.
- Android API 35 emulator smoke with the actual installed APK. Native startup, compact Sessions and toggled search, Settings, device rename, font and accent/background persistence across force-stop/relaunch, and opening the genuine HTTPS Tailscale sign-in page passed. No credentials were entered or enrollment performed. The final crash/JS/Go error log was empty.

Reproduce the Android smoke with `pnpm test:mobile:e2e --serial emulator-5554 --apk tmp/mobile-artifacts/shellfox-mobile-0.8.0-android-test.apk --browser`. It uses ADB and UI Automator, saves screenshots and redacted logs under `tmp/mobile-e2e/`, restores the device name and text size, and never signs into an account. A fresh emulator may need Chrome's first-run setup completed without an account.

Authenticated mobile-to-host terminal/session E2E still requires Tailscale enrollment and host approval. Chromium tests are not proof of Android/iOS IME behavior over a real remote terminal. Network loss, mobile process restart during a command, approval/revocation, and multiple clients need real-device acceptance. APK upgrade installation with a production signing key and newer published release remains unverified. No release was published. iOS compilation, simulator execution, device behavior, and signing require a Mac and remain unverified.
