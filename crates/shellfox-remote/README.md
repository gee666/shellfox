# Remote host boundary

This crate implements authenticated remote handlers and the bounded owner-channel boundary. The HTTP task does not open another Host, parse terminal output or launch a PTY. The real HTTP allowlist remains the five `/v1/{discover,pair,request,events,disconnect}` routes. Explicitly prepared observer-v2/manager-v2 signed handlers do not turn on V2 network ingress or default migration.

## Device policy and the native activation gate

The fixed approved-device policy is **Write + one explicit ApprovedFullAccount filesystem capability**, subject to the current account's OS permissions. There are no native device ReadOnly/Write choices, selected-folder grants or empty-roots inference. This device policy is not a Runtime controller lease, elevation grant or permission to operate a local-only source.

**Native persisted approval inventory is not runtime authority while approval/revocation storage remains gated.** This includes valid V2 records with no pending marker. Native `ApprovalControl::load` refuses grant installation; native approval-management calls return operational Unsupported/unavailable. If an already populated policy instance reaches that boundary, its authority is denied and retained file capabilities are revoked; enabling it again does not restore the old grants.

Readonly inventory remains separately available. The exact historical namespace is `remote-v1/approvals.json` and `remote-v1/write-pending`. Any pending-marker presence or read error denies loading; no marker deletion, flattening, hash-based commit inference or new marker reader is implemented. V1 normalization still requires saving before installation, and the gated save cannot satisfy that requirement. Records V2 and `control-full-account-v1` are unchanged.

The proposed versioned crash barrier is review-only: see [approval-crash-barrier-review.md](../../docs/approval-crash-barrier-review.md). Durable migration/history authority is unresolved. Neither a matching digest/UUID, current Go ownership nor visible endpoint publication establishes historical approval commit authority. No external monotonic authority or reapproval UX has been selected.

## Retained native preparation

The Service-private bootstrap receives the opaque `NativeControlChild` from the original acquired owner. It exposes only the derived anchored-relative `Arc<FileService>` to trusted persistence, never parent handles, issuer authority, paths or peer file-browser capabilities.

The original BootGuard stores `RemoteHostSetup::retained(child, &ownership)` **before** calling its borrowed `initialize(&mut self, helper, facts, resources).await`. Construction is infallible and performs no IO. The envelope retains the same ownership-loss token, root lifetime, creation keeper, typed preparation failures and publication attempts. Partial resources are stored before subsequent awaits. Initialization failure does not return an Err-only cleanup owner that the caller can accidentally discard.

The frozen availability contract distinguishes actual `RemotePrepared { dispatcher, control }` from `LocalOnly { reason: NativeApprovalStoragePending }`. The known static approval-authority gate produces LocalOnly directly before optional remote child/inventory IO or any remote bind/runtime/grants—not by catching an Unsupported/auth/IO error. The same acquired BootGuard starts one ordinary local Host with no remote control/dispatcher or sharing factory. `ReadyLocalOnly { remote_unavailable: reason }` is emitted only after actual local IPC/actor/endpoint activation, never as a transient Ready at the gate. This source migration does not approve remote authority or runtime acceptance. See [native-remote-availability-review.md](../../docs/native-remote-availability-review.md).

There is no path-only `PrivateApprovalStore::open`, `RemoteBackend::bind_owned` or `RemoteHostSetup::prepare` fallback. Rust preferences use the compiled scoped private-leaf read/publication operations. Reads check current Service ownership before and after access; the mutation fence is not a read-ownership or NoOwner guarantee. Failed/uncertain publications retain their exact attempt and quarantine replay. A visible preference publication does not claim power-loss durability or approval commit authority.

Native user-stage journals, uploads and editor initialization remain gated before IO. A native metadata cap is not recovery authority for absolute user-stage paths. Go library state/config remains path-based, separately unaccepted, and is not anchored by the Rust metadata capability.

## Owner integration and cancellation

`owner_channel(capacity)` returns an async `OwnerClient` and bounded `OwnerReceiver`. HTTP calls `RemoteHost::request`; the existing Service ownership loop dispatches the request. Each actual request future creates one canonical cancellation token, retains its drop guard across timeout/await, and passes the same token through `OwnerRequest::cancellation_token()` to every real Service commit site. There is no detached M2 cancellation model. A timeout never authorizes mutation replay.

Owner dispatch checks current approval and the complete profile/connection binding through synchronous commit. Runtime Claim/Observe admission uses the authoritative Service grant-custody path. Provisional mailboxes capture data/recovery but do not issue Release/Unobserve. The Service validates the exact actual offer and live book, then transfers central custody and commits the adopted book state in one owner-serialized operation. Cleanup retains exact leases across bounded queue pressure; no synthetic lease or ACK replaces missing proof.

For input only, an exactly validated historical completed/ignored receipt may finish after postcommit revocation. It carries its original binding/inputId/kind and no continuing grant. Uncertain writes and malformed/mismatched receipts return InputOutcomeUnknown with retryInput false, not a fake zero. Catalog and event results retain postcommit authorization checks.

## Facts and terminal limits

`RemoteDispatcher::with_facts_provider` borrows the existing Service. Its worker receives immutable snapshots/settings and actual runtime identities, never `&Service`. Collection is bounded; fresh metadata and runtime fences are checked before mutation. Targeted V1 operations do not collect unrelated unrepresentable tabs. V2 saved metadata retains authorized unknown facts rather than silently dropping rows. Local selection does not redirect remote input.

Generic V1 control must be judged by its actual adapter, not described as universally blocked by the native gate. `FormattedActiveBuffer` is limited recovery, **not FullState**, and does not establish full interactive parity. Observer delivery is intrinsically non-controlling; manager delivery associates one actual controller-or-observer role with separate control/layout references. Native source eligibility, elevated-policy exclusions, client capture and artifact/end-to-end gates remain separate. Pairing or DTO validation is not whole-app acceptance.

## Verification scope

Current restricted checks are metadata-only:

```text
cargo check --locked --offline -p shellfox-service -p shellfox-remote --lib --tests
cargo clippy --locked --offline -p shellfox-service -p shellfox-remote --lib --tests -- -D warnings
```

These commands compile tests; they do not link or execute the Service-lib test binary. Current instructions do not authorize Go/helper, network, PTY, Service-lib or native-runtime test execution.

Accepted in-memory policy/auth fixtures are explicitly separate from persisted native authority. The rewritten `private_approvals` fixture uses real Files native issuer/sealed receipts to test readonly inventory, legacy pending refusal, ownership loss and Unsupported/no-IO write gates. It is leaf-only, not Go/Service ownership or successful approval persistence proof. The private NativeBootGuard harness uses actual acquisition/control-child preparation but remains ignored and compile-only under current restrictions.

**Pending positive coverage:** approved durable approval persistence/restart recovery, native factory readiness and authorization, real controller/IfFree/takeover behavior, native platform capture/artifacts and live network acceptance. Earlier positive path-store/journal fixtures are not counted as passing these replacement gates. No obsolete ignored Go test command is a supported way to bypass the storage or runtime gates.
