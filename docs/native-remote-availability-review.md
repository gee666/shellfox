# Native remote gate and local availability

**The minimal typed contract is approved; source wiring is in progress, not runtime acceptance.** Native persisted approval authentication, pair, grant installation, ingress, file/input authority and approval-management activation remain absolutely gated.

## Audited behavior before typed availability wiring

For an explicit remote-composed boot:

1. `HostEndpoint::start_remote` selects `serve_remote_with_boot_report`.
2. `local::serve_boot_plan` acquires the original owner and creates one retained BootGuard.
3. `BootGuard::start_remote` awaits `prepare_remote_envelope` **before** calling `self.start(dispatcher)`.
4. The retained `RemoteHostSetup::initialize` reaches native `ApprovalControl::load`, which returns Unsupported even for valid V2/no-marker inventory.
5. That error propagates through `prepare_remote_envelope` and `start_remote`.
6. `serve_boot_plan` treats it as startup failure, cleans that same BootGuard and returns the error. No ServiceOwner/local endpoint readiness is reached.

This happens even when saved remote networking is disabled: the gate is reached before `RemoteRuntime::spawn`, which would otherwise load disabled intent. Thus the optional remote composition currently makes the entire local bootstrap unavailable.

The ordinary current desktop main is different: `main` → `app_startup::prepare` → `AppHost::prestart` → `HostEndpoint::start_with_dispatcher` → `serve_with_boot_report` with no remote options. Rust CLI `serve` calls `local::serve`, also without remote options. Those paths do **not** traverse this approval gate. This is a source-path finding, not runtime acceptance.

## Frozen minimal typed contract

`RemoteSetupOutcome` has exactly these cases:

- `RemotePrepared { dispatcher, control }`, containing actual prepared objects.
- `LocalOnly { reason: RemoteUnavailableReason::NativeApprovalStoragePending }`.

`LocalOnly` is produced directly at the known static authority gate after actual owner admission, before optional remote child/inventory IO, bind/runtime/grants. It carries no validated-inventory evidence. These are two distinct setup outcomes:

- **Remote prepared:** the existing dispatcher/control are actually ready, under their own accepted authority and cleanup conditions. Currently unreachable while native approval storage is gated.
- **Local-only / remote unavailable:** a specific typed reason, initially only `NativeApprovalStoragePending`, positively emitted by the static gate **before** remote bind, runtime creation, approval installation or capability enablement. This is not `Ok(empty approvals)`, a device ViewOnly mode or a successful remote setup.

Keep ordinary initialization errors separate. Do not convert AppError Unsupported, auth failure, timeout, unreadable endpoint or arbitrary setup errors into the local-only category. The known gate must produce the typed outcome directly. No error-text matching or generic catch is allowed.

## Approved local-only behavior

- Continue on the **same already-acquired BootGuard**, retaining the same NativeControlChild, root lifetime, creation keeper and unavailable remote envelope.
- Start exactly one ordinary local ServiceOwner/Host through that guard's normal local start path. Do not call another `serve`, release/reacquire, reopen another Host or cold-fallback.
- Bind/publish/activate only the normal authenticated local IPC endpoint. No remote HTTP listener, RemoteRuntime/Go node, remote dispatcher, approval Actor or peer filesystem/input capability is created.
- Keep remote source-sharing factory policy disabled. Do not install the requested remote sharing policy merely because local start succeeded.
- Local metadata, CLI and terminals may become ready only after their real normal local startup checks and endpoint activation finish.
- Remote actions must report typed operational unavailability. Do not attach a fake RemoteControl that returns empty approvals or a healthy disabled-remote state.
- Readonly stored inventory/preferences remain separately readable under the original retained capability and current-ownership read checks; they are not runtime authority or a prerequisite for pretending remote readiness.

The typed unavailable branch should be selected before private remote-child creation or other optional remote preparation IO. If an ordinary error occurs after a remote resource or uncertain publication is acquired, existing retained cleanup remains mandatory. That error must not be hidden as local-only availability. Any future post-effect downgrade would require separately reviewed proof that all remote runtime/listener resources were closed while the same guard and filesystem custody remain retained.

## Frozen status and cleanup contract

Use `LocalBootStatus::ReadyLocalOnly { remote_unavailable: reason }`. Common startup emits this final variant only after actual local IPC/listener, actor and endpoint activation. It must not emit a transient `Ready` first. Native/desktop/CLI waiters and getters must exhaustively accept local readiness while retaining/reporting remote unavailability; no fake RemoteControl is attached.

Endpoint publication/durability reports stay separate; visible/unconfirmed endpoint state is not approval authority. A local-only report is emitted only after actual local readiness, never at the early static gate itself.

Shutdown still uses the same BootGuard: deny authority, stop the one local actor, preserve/dispose of the same unavailable remote envelope and private creation custody, then confirm original helper ownership shutdown. Typed local-only availability is neither NoOwner nor cleanup confirmation. Fatal local startup errors retain the existing cleanup/reporting behavior.

## Required checks and acceptance gates

Write/compile tests for disabled and enabled remote intent, valid V2 inventory, legacy pending refusal, no remote factory/bind/runtime effects, one acquired owner/one Host, actual local readiness prerequisites, no observer factory activation, explicit remote unavailable status, fatal post-effect errors, cancellation and cleanup retention. Keep generic auth errors outside the typed local-only path.

Current permission remains source review and metadata checks only. No Go/helper, network, PTY, Service-lib execution or runtime permission follows from this proposal.
