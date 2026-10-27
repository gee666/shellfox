# Embedded mobile network client

This is a real Tailscale userspace node, embedded through Go `tsnet` and `gomobile`. It does not require the Tailscale app, a system daemon, an OS VPN, Android VpnService, or an iOS Network Extension. It only connects outbound to existing Shellfox hosts. There is no mobile host listener or loopback HTTP proxy.

## Build

From the repository root:

```text
node scripts/mobile-native.mjs setup
node scripts/mobile-native.mjs test
node --test scripts/mobile-native.test.mjs
node scripts/mobile-native.mjs build android
node scripts/mobile-build.mjs --test
```

The script uses `SHELLFOX_GO`, then `tmp/go-toolchain/go/bin/go`, then Go on PATH. Install Go 1.27.3 or newer. `setup` installs the pinned gomobile and gobind versions into `tmp/mobile-native/go/bin`. All caches and scratch files are under the root `tmp/` directory.

Android needs Java 17, Android SDK 36, build-tools 36.0.0, and NDK 27.1.12297006. Set `ANDROID_HOME` and `JAVA_HOME`. `ANDROID_NDK_HOME` overrides the native NDK selection. The AAR contains arm64-v8a and x86_64, matching the app's universal 64-bit APK. Minimum Android API is 24. Output is `tmp/mobile-native/android/shellfox-mobile.aar`.

On macOS with Xcode 16.1 or newer and the iOS device/simulator SDKs:

```text
node scripts/mobile-native.mjs setup
node scripts/mobile-native.mjs build ios
cd mobile/ios
pod install
```

Output is `tmp/mobile-native/ios/Mobile.xcframework` plus its local podspec. The Podfile links this generated framework; the Xcode project compiles `ShellfoxNetwork.mm` and `ShellfoxUpdates.mm`. Minimum iOS is 15.1. iOS compilation and device acceptance require a Mac; the Windows Android build does not validate them. Until iOS distribution is automated, manually update both Debug and Release Xcode settings when the root version changes. Current settings are `MARKETING_VERSION = 0.9.4` and `CURRENT_PROJECT_VERSION = 9004`, using the Android version-code formula `major * 1000000 + minor * 1000 + patch`.

## API and protocol

The gomobile package is `mobile`. Its generated Android class is `mobile.Mobile`; generated Objective-C entry points are `MobileStart`, `MobileStatus`, and so on. These names were checked against actual generated bindings.

Go exports:

- `Start(stateDirectory, hostname) error`, idempotent for the same active directory and hostname.
- `Status() (string, error)`, a JSON object with `status`, `loginUrl`, and `error`.
- `Login(authKey) error`. Empty string starts browser login; an enrollment key must start with `tskey-auth-`.
- `Peers() (string, error)`, JSON `{ "peers": [...] }`.
- `Request(peerId, path, body) (string, error)`, raw JSON response text.
- `Stop() error`, which preserves node identity.
- `Logout() error`, which logs out and stops. Call `Start` again before another login. A failed logout is reported rather than claiming credentials were removed.

`NativeModules.ShellfoxNetwork` exposes promise methods `start(hostname)`, `status()`, `login(authKey)`, `peers()`, `request(peerId, path, body)`, `stop()`, and `logout()`. Start and login resolve to status JSON text. Peers and request also resolve to JSON text; stop and logout resolve without a value. Native errors are intentionally generic. The renderer validates decoded JSON with shared schemas.

The native library shares health and bounded recovery code with `native/tailscale/nethealth` through a local Go module replacement. A rebuilt mobile native library must include that module. See its [health and recovery notes](../tailscale/nethealth/README.md). Missing mobile interface enumeration is treated as unknown, not proof that a live control connection is down. Local SDK control recovery preserves node identity, listeners and preferences and does not replay enrollment keys.

Login uses Tailscale's hosted coordination service. Open the validated `https://login.tailscale.com` URL from status in the system browser. Enrollment keys belong to the user, not an embedded app-wide credential. Tagged nodes and foreign-owner peers are excluded, so use an untagged user-owned enrollment. Host-side Shellfox approval remains required after Tailscale login.

Requests resolve a stable peer ID against the current network map, require an online, untagged peer with the same nonzero owner, then dial its Tailscale address on fixed TCP port 52743. Only POST `/v1/pair`, `/v1/request`, `/v1/events`, and `/v1/disconnect` are allowed. No caller-supplied target address, port, method, headers, or redirect is accepted. Policy matches `native/tailscale/helper.go` and `bridge.go`.

The current desktop host uses HTTP long-polling, not WebSockets. `/v1/events` holds a poll for up to 20 seconds. Go requests have a 30-second deadline, a 1 MiB JSON request limit, an 8 MiB JSON response limit, and a 16 KiB response-header limit. WireGuard encrypts the tailnet HTTP connection. No HTTP is sent through the platform fetch stack.

Each running node pools HTTP connections by the checked local and remote identities and target address. Ownership, tags and online status are checked on every request. Identity/address changes, unavailable ownership maps, stop and logout discard idle connections. Idle sockets expire after 15 seconds, before the helper's 30-second keep-alive deadline. Polls use independent connections while operations are active. POST bodies are not replayable, including after a stale pooled connection fails.

Polls and operations have separate admission limits, 64 and 12. Saturated calls reject immediately instead of queuing terminal mutations beyond the JS timeout. Android uses zero-queue executors; iOS uses nonblocking semaphore admission and concurrent dispatch. These are resource limits, not a UI host-count limit. Stop/logout cancel in-flight requests. A timeout after the host has accepted a mutation is still an uncertain result; it is not transaction rollback or permission to retry a destructive action.

## Storage and mobile lifecycle

Android SDK 30 and newer block Go's netlink interface enumeration. `AndroidNetworkProvider.kt` supplies actual SDK interface addresses and flags to Tailscale's `netmon.RegisterInterfaceGetter` before startup. The provider also supplies the default route and gateway; OS network callbacks trigger a fresh netmon check. This avoids both prohibited netlink access and synthetic interface guesses. The gomobile build enables cgo, so Android DNS uses the system resolver through Bionic rather than an absent `/etc/resolv.conf`.

Android stores node keys under `noBackupFilesDir`. iOS uses Application Support with backup exclusion and file protection until first unlock. The Go state store writes bounded, atomic, private files in the desktop-compatible format and refuses corrupt existing identity data. Enrollment auth keys are not persisted in app preferences. OS sandbox and device protection protect node keys; this is not a separate hardware-backed key store.

This is a foreground client. Locking the screen suspends JS polling and retries without withdrawing an approval request. The host retains a delivered request for one hour after its last pair attempt, or until approval, rejection or explicit client cancellation. No background heartbeat is required.

A foreground-started `/v1/pair` gets a bounded completion window. Android holds a partial wake lock for at most 35 seconds and releases it when the native request finishes. iOS requests a finite UIKit background task and ends it on completion or OS expiration. These do not guarantee delivery after force-stop, process death, loss of connectivity or an exhausted iOS background allowance. Undelivered requests retry on foreground resume; they cannot appear on the host before delivery. No foreground service or indefinite background network loop is used. Pending attempt UUIDs live only in JS memory. After process death, the old host request can remain for up to one hour; a new process cannot cancel that old UUID without a durable, identity-bound attempt store. The host can reject it, or let it expire. Cancellation is never broadened to every attempt from the device.

Do not stop the node merely because the app resigns active during browser login. Existing native polls and operations drain before JS starts another cycle. Bridge invalidation closes the node off the UI thread. Production acceptance must cover Wi-Fi/cellular changes, device sleep, browser login/resume, expired keys, logout, revocation, and host approval on both platforms.

Generated dependency notices ship in Android `assets/notices` and the iOS `ShellfoxNetworkNotices` resource bundle. See `NOTICE-AUDIT.md` before redistribution.
