# Observer-v2 isolated Android runtime fixture plan

Status: PLAN ONLY. No emulator, AVD creation, ADB server, device inspection or install is authorized by this document. Review is required before starting. Desktop UI pause remains absolute.

## Inputs and provenance

Use only the verified compiled bundle `tmp/observer-v2-artifacts/860d3aa1c8f4a5f5c234a4058156cc77e1da54e87b9251a8ba3de14595f8a50f/` and its authoritative snapshot/manifests. Preserve the 477 old V1 files, snapshot `91ed…04964`, and all existing bundle artifacts.

The six earlier V1 fixtures do not cover this test matrix. New test-only fixtures need a separate quiet overlay manifest/hash. Never claim that overlay was part of source snapshot 860d. The candidate fixture APK and test APK must record production snapshot identity, overlay identity, actual archive/header/tool hashes and their own APK hashes. A test-only rebuild does not relabel old artifacts or live HEAD.

The fixture package will be unique, such as `com.shellfox.mobile.observerfixture.r01`, with a separate test package. Use a plain Application, no launcher, no React Native app startup, no Mobile.start/login/logout, no Tailnet, no real pairing, no host listener and no PTY. Remove INTERNET, ACCESS_NETWORK_STATE, ACCESS_LOCAL_NETWORK and overlay permissions from merged manifests and verify the built APKs before installation.

## Original-process and transport isolation

Proposed run identity is `observer-860d-r01`. All run files belong under `tmp/observer-v2-runtime/860d3aa1c8f4a5f5c234a4058156cc77e1da54e87b9251a8ba3de14595f8a50f/<overlay-hash>/r01/`.

- Fresh AVD name `sf_observer_860d_r01`; never reuse an existing AVD or userdata.
- Only the existing AOSP `android-35/default/x86_64` image, revision 2.
- Proposed private ADB port `31603`, emulator console port `5592`, emulator ADB port `5593`, exact serial `emulator-5592`. Port conflicts abort; never adopt a listener or auto-select another device.
- AVD_HOME, USER_HOME, EMULATOR_HOME, generated ADB keys, console token, temporary files and userdata all point inside that new run directory. No user keys or global auth files may be read/copied.
- Private ADB server only. Restrict USB handling to a deliberately nonexistent fixture-only USB identity, never scan/select a physical device. Every client operation names the private host/port and exact owned serial. Never use unqualified `adb`, `devices`, `connectedAndroidTest` or the global server.
- Launch and hold original process objects/handles. Record executable path/hash, PID and OS creation identity at spawn for both the private ADB server and emulator/qemu chain. PIDs cannot be known before launch and must not be invented. A PID alone is never cleanup authority.
- Existing WHPX only, two vCPUs, 2048 MiB RAM, headless, software graphics, no audio/cameras, no snapshots, fresh disposable userdata. No driver, feature, reboot, firewall or system SDK changes.

## Blocking network-isolation proof

Only owned loopback ADB/console traffic is allowed. Fixture manifest permissions and injected transports block application network use, but are not a host-process or guest-network sandbox.

The earlier SDK workflow showed that ADB can open mDNS UDP sockets even with auto-connect disabled. A fresh AOSP image may also have boot-time network services. Neither an empty account store, `ADB_MDNS_AUTO_CONNECT=0`, a post-boot airplane-mode setting, nor an unverified QEMU `-net none` suffix proves no non-loopback traffic from process start.

Before any startup, establish from the installed pinned tools a supported, verifiable way to disable both emulator guest/host egress and ADB discovery/egress from process creation, while retaining only the private loopback channel. Do not invent flags, assume defaults, or enable global host controls. The review must name the exact supported mechanism and how its effective configuration is checked without allowing a race before isolation.

This guarantee is not yet established. Therefore the plan is BLOCKED and nothing will run. If the existing toolchain cannot provide it without forbidden host changes, report that prerequisite and keep the gate closed. A packet trace after boot is not a substitute for a startup isolation guarantee.

## Test-only native transport

Load the actual new Rust/JNI and Go-linked module from the compiled gate. Go library loading must not start tsnet. Inject a test-only responder through the real native dispatch callback. Use the actual immutable RequestResult/status completion contract and canonical golden JSON, not a live endpoint or English-error parser. Record that network/Go request functions were never used for dialing.

All handles must come from real JNI creators. Run engine create/reset/pump/frame/consume/close on the recorded serial owner OS thread. Inspect native parser progress and ACK requests, never assert receipt as consumption. No JS per-cell/frame path or fabricated V1 DTOs.

## Runtime matrix

1. V1 creator remains default, API profile 1/caps 7, old fixtures/layouts and legacy completion unchanged.
2. Explicit observer creator reports extension ABI 1/wire 2, profile 2/caps 1. Capability bits never become approval/input/layout grants. No discover/pair occurs on creation.
3. Complete a pending DiscoverV2 with actual numeric HTTP 404 and the empty non-2xx body. Only the reviewed Rust negotiation may offer legacy. Assert no V1 pair/attach/control follows. Auth/approval failure, business UNSUPPORTED, malformed response, timeout/cancel and English errors cannot create synthetic 404 or fallback.
4. Bind real observer delivery to a canonical grid different from the local viewport. Apply exact-ticket reset and full recovery. Set the UI read-only prop false and vary viewport/font size. Input, takeover, upload and host layout remain denied; parser dimensions never local-fit. Clip/pad locally only.
5. Before pump or before complete recovery consumption, assert no recovery/data ACK. After full owner-thread parse, assert one correctly stamped recovery ACK at its fixed boundary. Data ACK requires all bytes through its received sequence. Test partial pump, duplicate ACK preparation, layout boundary/gap, stale stamp and revoked credit.
6. SourceEnded remains source end, never invented ProcessExited, exit code, cleanup authority or timestamp. Validate a separately evidenced ProcessExited fixture distinctly.
7. Reconnect with a new explicit identity fences old deliveries, callbacks, ACKs and registry tickets. No automatic mutation retry or V1 pairing. Test cancellation preserving unrelated same-cycle work, late pending pair withdrawal, bounded disconnect completion, close and late completion rejection.

## Collection and cleanup

Use bounded per-operation and total deadlines. Retain input/overlay/artifact hashes, original-process identities, exact effective flags, instrumentation output, fixture method/ACK assertions, API/page-size facts and sanitized diagnostics.

In a finally path, stop only the verified owned emulator via its explicit serial/console, wait for original process exit, then stop only the verified private ADB server. Verify all three owned ports are closed and owned handles exited before removing disposable AVD/userdata/keys. Do not kill by process name, numeric PID fallback or a global server command. Cleanup uncertainty is a failed gate with ownership retained, not permission to affect another process.

API 35 with 4 KiB pages proves neither API-24 device runtime nor 16-KiB-page runtime. Even a passing backend matrix does not accept real IME/Canvas/TalkBack/TUI behavior, desktop UI or overall mobile adoption.
