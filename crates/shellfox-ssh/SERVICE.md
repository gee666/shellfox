# Native service SSH assembly

The new `crates/shellfox-service/src/ssh*.rs` modules own profile operations, connection preparation, native prompts, private askpass IPC and SSH status. They do not open another metadata Host, start a second Runtime or emulate terminal output. The service module declaration and its direct dependencies are now installed and compile in the actual service crate. The protected native owner install/dispatch/lifecycle hooks and concrete same-Service runtime adapter are now wired. The application's verified resource-policy composition and GPUI presentation bindings remain consumer responsibilities.

## Construction and authority

ModuleManager supplies one `Arc<shellfox_ssh::PrivateDirectory>` for `<ProjectUserRoot>/rust-service-v1/ssh`. Open it with the existing verified Go helper policy on Windows. Reuse that exact capability for `ProfileStore`, keys and `SystemSshAdapter`; `SshManager::new` rejects a different adapter directory. It accepts a configured askpass executable, read-only PuTTY import provider and the final native ownership-loss token. Interactive file dialogs stay in the local GPUI client. No source request chooses an executable, private directory or helper permission policy.

The owner injects the application validator and runtime starter:

- `NativeControllerValidator::validate_native(controller)` checks current native application authority. `validate_target(controller, tab)` checks tab existence/application permission, never the input lease. Pending auth does not need a PTY claim, and input takeover does not revoke SSH. Guarded profile/trust mutations recheck after lock waits.
- `OwnedRuntimeStarter::reserve_target(controller, SourceConnectTarget)` atomically reserves the target before preparation. Failure must leave no reservation. The opaque target carries connection/tab getters but has no public constructor or wire representation. The same owner's local attach/start paths must reject auto-local-shell creation while that source is reserved. `SshManager::source_target` and `ensure_local_start_allowed` expose the guard; the parent must wire it before publishing the source handler.
- `OwnedRuntimeStarter::start(controller, target, &SshLaunchRequest, &local_environment)` requires that exact reservation, then registers and starts one trusted custom `PtyCommand` in the existing runtime. Returning an error means nothing was registered or spawned. If a started runtime is draining or failed, return its Identity for retained cleanup. Keep the existing opaque trusted Program checks; do not turn a request pathname into unrestricted execution authority.
- `close(identity)` returns true only after confirmed original-generation cleanup. `release_target(target)` then releases only the matching source after worker/broker/identity cleanup. `runtime_is_current(identity)` is a read-only active-generation check for Ready adoption, never an implicit input claim.

The runtime crate has no dependency on SSH crypto. The service assembly owns conversion, proof matching and authentication state.

## Native request API

`NativeAction::Ssh` carries `ssh::Request` and returns `NativeResponse::Ssh`. `NativeAction::SshOpen(OpenProfile)` returns `NativeResponse::SshOpened`; the owner allocates no local-shell creation facts for that original intent. Trusted bootstrap installs once with non-serializable `OwnerClient::install_ssh_native`. Desktop settings adapters translate their existing `settings::api` fields into these types. Requests are not serializable mobile/network commands. `Input` and `Password` deliberately have no Debug implementation. Request Debug prints only variant names, never payloads.

```text
List
Save(Input)
Delete { id: Uuid }
Import
ValidateKey { selected: SelectedKey }
GetStatus { connection: Option<Uuid> }
Connect { profile: Uuid, tab: TabId, cols: u16, rows: u16 }
ConfirmHost { connection, challenge, decision }
AnswerPrompt { connection, prompt, answer: Option<Secret> }
Disconnect { connection }
AdoptReady { connection }
```

`Password::{Keep, Clear, Set(Secret)}` preserves omitted/null/string behavior, including an empty replacement. The UI may translate its empty editor field to Keep, but the native API does not conflate an explicit empty password with Keep.

`Response` contains local sanitized Profiles, Imported counts, SelectedKey information, Status rows or Applied. After an explicit local dialog, GPUI constructs `SelectedKey::new(absolute_path)` and sends ValidateKey. Cancellation stays client-side. The server checks private ownership/ACLs and returns path, format metadata and `cryptographically_validated`. That flag is true only after actual private-key parsing/consistency validation succeeds; encrypted or explicitly unsupported legacy candidates remain false. There is no server-side chooser RPC or actor waiting on a modal dialog. Remote-only mobile clients continue to receive the stricter remote DTOs, never Request/Input, saved credentials, key paths or broker capabilities.

## Preparation and runtime lifecycle

`manager.request(controller, request, validator, starter)` runs on the native owner. A bounded preparation worker performs public probing and key conversion outside that owner loop. Unknown host keys produce `State::AwaitingHost` with a challenge ID, host, port, algorithm and SHA256 fingerprint. Only a matching, currently authorized ConfirmHost resolves that callback. Changed/corrupt trust is an error, not a replacement prompt. Prompt answers are also bound to the connection, prompt ID and controller.

Poll `manager.poll(validator, starter)` on the same owner tick. It consumes worker events, starts the private broker, rechecks authority and passes the prepared command into the existing runtime. Status becomes Authenticating, not Ready. Forward actual raw bytes from that runtime through `observe_output(identity, bytes)` before/alongside ordinary terminal delivery. This is a bounded marker matcher, not another terminal parser. A marker from another generation does not count.

The remote command emits an unpredictable OSC marker only after the pinned OpenSSH handshake authenticated and opened its command channel. The existing Ghostty terminal still receives the original bytes. Native/UI messages cannot set Ready.

Ordinary `detach_controller` cancels pending auth/prompts. An authenticated Ready process remains host-owned and survives view detach or input takeover. A new authorized application view may discover Ready status and explicitly `AdoptReady`; adoption rechecks native permission, target permission and the current generation, but never claims input, resizes or starts a runtime. Stale or unauthorized adoption fails. The normal terminal attach/claim path remains separate.

Explicit `revoke_native_capability(controller)`, Disconnect, native ownership loss and application stop request original-runtime cleanup, including Ready connections. Update authoritative native grants when reducing security; do not use ordinary input/view detach as that revocation signal. `begin_shutdown` closes every connection and stops new admission. The manager keeps broker, encrypted identity and source reservation until cleanup is confirmed. Forward `runtime_closed(identity)` after confirmed cleanup if Service removes that runtime from its registry; missing state alone must not imply confirmed cleanup. Call `begin_shutdown`, continue polling, then require `is_closed()` before dropping the module. Accidental drops retain unconfirmed resources in a process-lifetime hold rather than release them beneath a live SSH process. Normal shutdown must not rely on that hold.

## Private askpass

The broker reuses Service's private Unix socket/Windows named-pipe listener and same-user peer verifier. Windows pipes inherit the proven private directory DACL and reject remote clients. Frames have a 512 KiB cap, prompt text has a bounded presentation form, and account credentials travel only in zeroizing private IPC buffers.

The child's local environment contains the explicitly configured and role-approved askpass program path, endpoint/directory and a random IPC capability. `SshProgramRole::Askpass` is distinct from SSH/Keyscan approval; neither current_exe nor an acceptance EXE is inferred as a production resource. That capability is not an account password or a key decryption passphrase. It never enters OpenSSH SetEnv/SendEnv. No account credential is put in argv, environment variables, public files or terminal output. The stored login password is tried once; later/unknown prompts require explicit native UI answers.

CLI and Desktop call the ONE shared `ssh::dispatch_early_entry(args)` before normal argument parsing, logging, Tokio or a UI. Without target callback footprint it delegates to the existing authenticated PTY/raw supervisor entry points and preserves normal diagnostics. With callback footprint it accepts only one arbitrary prompt and authenticates through the private broker; prompt text equal to either supervisor flag is still a prompt. Partial/mixed/extra-argument callback contexts fail silently with code1, not normal UI fallthrough. The footprint is never executable trust. Full raw u32 exit bits are preserved. The lower-level `askpass_entry` writes an answer only to SSH-owned captured stdout; already-async configured helpers can use `askpass_from_environment`.

PPK/PKCS8 keys are unlocked in memory and re-encrypted as an OpenSSH `PreparedIdentity` with a fresh random passphrase. The manager retains that guard until confirmed shutdown, then removes only its private encrypted artifact. An exact local key-passphrase prompt can receive this temporary credential. Keyboard-interactive is disabled in this prepared-key mode because a server-controlled KI prompt could otherwise impersonate a local key-unlock prompt. Full prepared-key plus KI parity needs an owned agent-loading route or a reviewed auth library; it is not silently declared supported.

## Evidence and remaining integration

Current actual service lib+tests typecheck and `--lib --tests` Clippy with warnings denied pass. The default SSH unit slice passes 18 tests with one explicit-artifact native case ignored by default. That native case was separately executed and passed on Windows using one real owned Runtime/PTY, a private supervisor and a std-only fake SSH target. It verifies pending input-control denial while readonly Observe remains allowed, positive proof-backed Ready control admission, stale/revoked denial, activity/input settlement, detach/adoption and original cleanup. The public `ssh_lifecycle_contract` target also passes three tests, including its runnable fixture entry. Historical checkpoints had 13 and then 15 SSH tests; those are not the current count. A vendor portable-pty unused-constant dependency warning remains. No scratch harness remains. SSH tests cover native CRUD/revocation, exact trust challenges, cleanup retention, failure before proof, wrong IPC capability rejection and a local fake SSH subprocess that obtains its synthetic password through the private broker and emits the remote-command marker. No real SSH/keyscan executable, network host, account, production key or user configuration was used.

The public synthetic-process fixture alone is a contract check. The separately executed native case exercises the actual Service attach/control guard and original owned Runtime/PTY path. The native actor, local-start reservation guard and input-control gate are now wired; the trusted target-only askpass carrier leaves ordinary user environment protection intact. Production verified-resource composition and GPUI-local dialogs/presentation still need application acceptance. Linux/macOS and Windows arm64 have not been exercised by this worker. Legacy DSA, traditional encrypted PEM cryptographic support and prepared-key KI policy remain explicit acceptance items. Genuine SessionCreationFacts describe original intent and must be preserved on existing sessions. New arbitrary-program SSH sessions use None creation facts; current local SSH executable/unsupported process inspection must stay separate from remote shell facts, without dropping live-output TTL/input activity hooks. Their selected material/metadata can be retained without pretending those operations work.
