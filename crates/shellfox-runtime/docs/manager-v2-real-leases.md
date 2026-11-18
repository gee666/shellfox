# Manager-v2 real-lease proposal

Historical pre-code review input, superseded by Russell's canonical manager-v2 contract and its approved amendments. Do not implement this draft's competing wire samples. The actual additive Runtime producer is now implemented and documented in [controller-stamps.md](controller-stamps.md); it adds no UI, wire or permission policy.

## Decision and unchanged boundaries

Add the `manager-v2` negotiated profile, not a mode switch inside an existing observer attachment. Keep v1 and `observer-v2` JSON and behavior unchanged. Discovery may advertise `manager-v2` only when its complete handlers are installed. Pair offer, selection and every bound request must agree on the profile. Unknown profile/method/field combinations fail validation; no fallback to v1 write APIs.

A manager delivery owns exactly ONE real Runtime delivery association. If its Claim succeeds, that association is the returned controller `Lease`. Otherwise an explicitly read-only attachment uses a real `ObserverLease`. A controller delivery never allocates a parallel observer and never ACKs a hidden delivery. Runtime keeps its current combined controller input/output ownership; no core separation is proposed.

Effective access still requires the captured trusted LOCAL `ExistingManagerStandardCatalog` opt-in for this generation, actual Started success, actual sealed KnownNormal, current approval and the corresponding current peer permission. Unknown/Elevated veto eligibility becoming effective. Unsupported OS stays Unknown. WSL, guest, SSH and raw remain LocalOnly without separately approved policies. A delivery/control ref is not ShareGrant, native ownership, metadata authority or an elevation override. Metadata visibility is checked before returning IDs.

## Private associations, not client assertions

`ServerDeliveryRef` is server-only, keyed by authenticated peer + connection binding + wire delivery ID:

```rust
struct ServerDeliveryRef {
    identity: Identity,
    reference: DeliveryRef,
    actual: ActualDelivery,
    policy_epoch: u64,
    phase: AwaitingRecovery | Live | Revoked,
}
enum ActualDelivery {
    Controller(Lease),
    Observer(ObserverLease),
}
struct ServerControlRef {
    identity: Identity,
    reference: ControlRef,
    delivery_id: Id,
    actual: Lease,
    policy_epoch: u64,
}
```

The displayed types are proposed shapes, not compilable declarations. A private enum carries phase-specific recovery/credit state. Never accept `actual`, numeric controller/viewer identity, policy epoch or Runtime recovery ID from JSON.

- Controller `DeliveryRef.deliveryEpoch` is the actual returned `Lease.epoch`.
- Observer `DeliveryRef.deliveryEpoch` is the actual returned `ObserverLease.epoch`.
- `ControlRef.controlEpoch` is that SAME controller lease epoch, not a new authority counter.
- Random delivery/control IDs are independently allocated, connection-scoped lookup keys. All ref fields must match the stored association. IDs/epochs alone are not authentication.
- Ref allocation and credit remain bounded. Runtime counters outside the JSON-safe integer range fail unavailable; never truncate or wrap.
- No client chooses controller vs observer by editing a ref. The server records which actual lease was granted. Every operation checks ControlRef's actual lease equals its associated delivery's Controller lease, not merely equal epoch numbers.
- Controller delivery requires current read AND write permissions. Losing write revokes both its control and delivery refs even if read remains. The peer must explicitly attach a new observer and consume fresh recovery. Observer delivery requires current read permission; metadata authority remains separate and is checked first.

## Concrete wire delta

New manager-only structs use camelCase and deny unknown fields. Reuse existing UUID validation, base64 decoding, data/recovery byte limits and formatted-active-buffer-v1 limitations. ManagerRecovery keeps the same field names but uses native grid validation: axes 1..32767, product within the source's configured GridLimits and absolute 1048576-cell cap. Do not change observer-v2's existing 2..500 validation or clamp the real grid. All nullable fields below are required, not omitted defaults.

| Manager-only type/method | Required fields and result |
| --- | --- |
| `ControlRef` | `tabId`, `generation`, `controlId`: non-nil UUIDs; `controlEpoch`: positive safe integer. |
| `attachTerminal` | `tabId`, `resume`: existing nullable Resume; `controlIntent`: `observe` or `claim`. A live controllable source uses actual Runtime Claim with takeover=false; ended/intrinsic read-only sources use an actual observer. |
| Attached result | `kind:attached`, `delivery`: existing DeliveryRef, `input`: ControlRef or null, `controlState`: `granted`, `blocked`, `intrinsicReadOnly` or `notRequested`, `layout:{kind:fixedCanonicalGrid}`, `recovery`: ManagerRecovery with the existing field shape and native grid bounds. |
| `takeTerminalControl` | `delivery`: current OBSERVER DeliveryRef, `takeover`: explicit boolean. Returns `kind:granted` with a COMPLETE replacement attachment, or `kind:blocked` with the unchanged observer delivery. A controller association rejects `alreadyControlled` without a second Claim. |
| `recoverTerminal` | `delivery`: current DeliveryRef. Returns ManagerRecovery, from the exact mapped actual lease. |
| `acknowledgeTerminal` | `delivery`, `ack`: existing data/recovery union. Recovery ACK adds required `layoutRevision` in this manager-only schema. No scalar-only recovery ACK. |
| `detachTerminal` | `delivery`. Invalidates related control ref too, then retires its actual lease without killing the process. |
| `writeTerminal` | `control`, `inputId`: UUID, `encoding:base64`, `data`: canonical base64, `captureHints`: object below. Returns correlated AcceptedInput. |
| `structuredMouse` | `control`, `inputId`, `event`, `geometry`, `captureHints`. Returns the same correlated AcceptedInput shape. No mouse escape-string field. |
| `resizeTerminal` | `control`, `cols`, `rows`: axes 1..32767 and product within configured/hard GridLimits, never a client-provided cell budget. Returns applied dimensions/revision; this is NOT recovery consumption or delivery permission. |

`DeliveryRef` retains exactly `tabId,generation,deliveryId,deliveryEpoch`. Existing observer-only `Attachment.input:null` is not widened in place. Manager has its own attachment/request types and negotiated method allowlist. Existing observer profile still denies control/write/resize.

Check current write authority BEFORE a requested Claim, and separate takeover authority before takeover=true. Missing authority is a permission error, not a control-blocked fallback; no native Claim is sent. Read-only callers may explicitly use controlIntent=observe. Only an actual non-takeover Claim denial because another controller holds the source is `blocked`. Recheck live controllability before issuing ControlRef after Claim; a source ending during attach is retired/re-attached through the real observer path, not advertised as input-capable because legacy Runtime Claim accepts final-screen readers.

`resume` remains a generation/cursor consistency hint, not authority to skip full recovery. Initial attach, observer-to-controller promotion and any replaced real lease always return a fresh stamped full recovery in this first manager slice.

### Attach payload and actual controller result

The request envelope stays `{version:2,binding,method,payload}`. Its binding profile is `manager-v2`; transport-authenticated peer checks remain mandatory. The JSON below is the method payload/result, not a new unbound endpoint.

```json
{"tabId":"11111111-1111-4111-8111-111111111111","resume":null,"controlIntent":"claim"}
```

```json
{
  "kind":"attached",
  "delivery":{"tabId":"11111111-1111-4111-8111-111111111111","generation":"22222222-2222-4222-8222-222222222222","deliveryId":"33333333-3333-4333-8333-333333333333","deliveryEpoch":7},
  "input":{"tabId":"11111111-1111-4111-8111-111111111111","generation":"22222222-2222-4222-8222-222222222222","controlId":"44444444-4444-4444-8444-444444444444","controlEpoch":7},
  "controlState":"granted",
  "layout":{"kind":"fixedCanonicalGrid"},
  "recovery":{"recoveryId":"55555555-5555-4555-8555-555555555555","format":"formatted-active-buffer-v1","encoding":"base64","data":"GBtj","lastSequence":42,"cols":120,"rows":32,"layoutRevision":3}
}
```

`GBtj` illustrates only the mandatory CAN+RIS prefix. A real response contains the actual complete bounded formatter result. It is not a full-state/binary snapshot claim.

If Claim is explicitly blocked, the same result shape has `input:null`, `controlState:blocked`, a NEW delivery ID and the actual observer epoch, with recovery captured through that observer. Intrinsic read-only sources use `intrinsicReadOnly`, but this does not override the current LocalOnly policy. Other Claim errors or uncertain outcomes are NOT classified as ordinary blocked. They fail closed with retained owner/correlation state for reconciliation, not a second delivery.

### Recovery ACK payload

```json
{
  "delivery":{"tabId":"11111111-1111-4111-8111-111111111111","generation":"22222222-2222-4222-8222-222222222222","deliveryId":"33333333-3333-4333-8333-333333333333","deliveryEpoch":7},
  "ack":{"kind":"recovery","recoveryId":"55555555-5555-4555-8555-555555555555","sequence":42,"layoutRevision":3}
}
```

Wire recovery UUID maps privately to the exact opaque Runtime RecoveryId, actual lease, sequence, dimensions and revision. Do not synthesize `Ack(sequence)` for controller recovery. Route to the dedicated controller stamped ACK below. A wrong, superseded, foreign or duplicate recovery ACK does not free credit or release a geometry barrier. Ordinary data ACK is allowed only for an actually offered boundary on that ref and never consumes a pending recovery.

Exact routing after resolving the private association:

| Operation | Controller association | Observer association |
| --- | --- | --- |
| Recovery | `RecoverStamped { lease }` | `ObserverRecoverStamped { observer }` |
| Full recovery ACK | `ControllerLayoutAck { lease, exact stamp }` | `ObserverLayoutAck { observer, exact stamp }` |
| Offered data ACK | `Ack { lease, sequence }` | `ObserverAck { observer, sequence }` |
| Bytes/layout/end | actual `Output`/`ControllerLayout`/`Exited` for that Lease | actual `ObserverOutput`/`ObserverLayout`/`ObserverExited` for that ObserverLease |
| Detach | invalidate refs, then `ReleaseControl { lease }` | invalidate ref, then `Unobserve { observer }` |
| Input/resize | separate ControlRef resolving to SAME Lease | denied, never translated into a controller request |

## Additive Runtime controller API

Current observer APIs already provide stamped recovery/layout gating. Current controller `Recover` returns only ANSI + sequence, and `Ack` can consume its scalar recovery barrier. They are insufficient for manager-v2. Add these working native handlers together, leaving legacy commands/events unchanged:

```rust
Command::RecoverStamped { lease: Lease, capability: AnsiRecoveryCapability }
    -> Response::Recovery(StampedRecovery)
Command::ControllerLayoutAck {
    lease: Lease, recovery_id: RecoveryId,
    last_delivery_sequence: u64, layout_revision: u64,
} -> Response::Applied
Event::ControllerLayout { lease: Lease, layout: LayoutStamp }
Command::MouseStamped {
    lease: Lease, event: MouseEvent, geometry: MouseGeometry,
    stamp: ControllerGeometryStamp,
} -> Response::Input(InputReceipt)
struct ControllerGeometryStamp {
    recovery_id: RecoveryId, layout_revision: u64, cols: u16, rows: u16,
}
```

- `RecoverStamped` validates the REAL current controller lease, captures bytes/sequence/dimensions/revision/ID in one owner-thread barrier, resets its real delivery credit and selects stamped gating for THIS epoch only.
- Only `ControllerLayoutAck` with the exact latest full snapshot ID/sequence/revision consumes that recovery. Ordinary `Ack`, `Replay`, legacy `Recover`/`RecoverAnsi` cannot downgrade a stamped epoch or bypass it. They return validation while stamped gating is selected. V1 epochs that never opt into stamping keep existing behavior.
- `ControllerLayout` is ordered after earlier bytes and before bytes parsed at the new canonical dimensions. It blocks subsequent delivery until a fresh full recovery is actually consumed. No resize-only ACK. Layout journal loss requires full recovery, not a tail skip.
- A resize between snapshot and ACK leaves a subsequent geometry barrier. Replacement recovery supersedes its predecessor even if sequence/revision happen to match.
- Claim replacement/release removes the previous controller fence. Observer/controller recovery IDs and ACKs are not interchangeable. At most one stamped controller plus the existing 16 observers; no extra replay bank or parser.
- Queue-full notification/data commits no credit or progress. Reuse the existing canonical layout journal and recovery counter, generalized by private lease variant rather than copying a second geometry authority.
- Stamped state retains the last ACCEPTED recovery ID for mouse validation, separate from any pending replacement. A pending or resized barrier rejects stamped mouse input until recovery is accepted.

Two additional lifecycle gaps need explicit native handlers, not a Service-only fake lease:

```rust
Command::ReleaseControl { lease: Lease } -> Response::Applied
Event::ControllerRevoked { lease: Lease, reason: Replaced | Released }
```

Release checks the actual lease, clears controller credit/initialization and invalidates it without closing PTY/source. The next Claim receives a fresh real epoch. Claim replacement/release emits an ordered revocation before delivery under the replacement epoch. These are additive; v1 callers need not invoke release or consume manager refs. No existing v1 shutdown/cleanup semantics change.

## Binary input, capture and acceptance

`writeTerminal.data` decodes to 1..65536 bytes before Runtime submission. No UTF-8/high-byte string round-trip, JS code-unit reconstruction, newline normalization or client-side terminal-mode encoding. The manager Service adapter may call existing `RawInput` only AFTER resolving its current ControlRef to that same actual controller lease and checking current write permission. Runtime still independently validates the lease and complete write. A wire ControlRef is not passed into Runtime as a lease.

`captureHints` is required and discriminated:

- `{kind:"captured",userInitiated:boolean,hint:boolean,settledAt:null|timestamp}` preserves the original client-admission marker, including explicitly captured null. Validate timestamps and the separate authority for that hint. Never recapture current settings/settlement while queued or on completion.
- `{kind:"unavailable",userInitiated:boolean}` carries no captured marker or unsettle request. It does not invent captured null or enable legacy missing-marker behavior. Delivery/write permission does not grant metadata access.

Concrete internal delta needed for the second form: add host/admission `CapturedSettlement::Unavailable` with an explicit no-unsettle handler. Keep existing `Legacy` behavior restricted to existing legacy callers; manager never maps unavailable to Legacy. Meaningful input/activity still comes from actual accepted bytes/native event mechanics, not the hint. Metadata failure cannot retract/retry an accepted PTY write.

```json
{
  "control":{"tabId":"11111111-1111-4111-8111-111111111111","generation":"22222222-2222-4222-8222-222222222222","controlId":"44444444-4444-4444-8444-444444444444","controlEpoch":7},
  "inputId":"66666666-6666-4666-8666-666666666666",
  "encoding":"base64","data":"gAD/DQ==",
  "captureHints":{"kind":"captured","userInitiated":true,"hint":false,"settledAt":null}
}
```

This payload is four binary bytes, including high bytes and NUL. The receipt contains no copy of them:

```json
{
  "kind":"accepted",
  "control":{"tabId":"11111111-1111-4111-8111-111111111111","generation":"22222222-2222-4222-8222-222222222222","controlId":"44444444-4444-4444-8444-444444444444","controlEpoch":7},
  "inputId":"66666666-6666-4666-8666-666666666666",
  "inputKind":"binary","acceptedBytes":4
}
```

Allocate a server Runtime RequestId, reserve the existing bounded input-admission slot before enqueue and associate `(connection,identity,actual lease,inputId)` with that exact RequestId. Emit AcceptedInput only for its matching `Response::Input`, not queue admission or a transport ACK. Nonzero means the complete encoded write, NOT program execution. `inputKind` is exactly `binary` or `mouse` in this slice, selected from the matching native receipt path, not a client assertion. Mouse can legitimately return acceptedBytes=0. AcceptedInput has required nullable `control`; return null if that ref has since been revoked, never issue a replacement ref in a receipt. A known accepted outcome remains correlated by inputId without echoing source IDs when metadata authority is no longer current. Never echo password/input/context in receipt or logs.

Duplicate in-flight inputId fails without a second write. A bounded completed-ID book may return the same recorded result without rewriting; an outcome-unknown request is retained as unknown and never automatically resent. After eviction there is no exactly-once guarantee: clients must not retry an unknown input on timeout/reconnect/new epoch. A failure after possible partial I/O is `outcomeUnknown`, not acceptedBytes=0 or a safe retry claim.

Manager errors reuse the existing uppercase application code set but have a separate manager-only detail union. They do not widen observer-v2 error JSON:

```json
{"code":"TARGET_LOST","message":"Mouse layout changed; consume fresh recovery.","retryable":false,"detail":{"kind":"staleLayout","recoveryRequired":true}}
```

```json
{"code":"NATIVE_UNAVAILABLE","message":"Input outcome is unknown; do not resend.","retryable":false,"detail":{"kind":"inputOutcomeUnknown","inputId":"66666666-6666-4666-8666-666666666666","retryInput":false}}
```

No error includes invisible source IDs. `recoveryRequired` invites a separate recovery operation; it never authorizes replaying the input event.

## Structured mouse payload and owner checks

```json
{
  "control":{"tabId":"11111111-1111-4111-8111-111111111111","generation":"22222222-2222-4222-8222-222222222222","controlId":"44444444-4444-4444-8444-444444444444","controlEpoch":7},
  "inputId":"77777777-7777-4777-8777-777777777777",
  "event":{"action":"press","button":"left","modifiers":{"shift":false,"ctrl":false,"alt":false,"super":false},"xPx":48.5,"yPx":27,"anyButtonPressed":true},
  "geometry":{"recoveryId":"55555555-5555-4555-8555-555555555555","layoutRevision":3,"cols":120,"rows":32,"cellWidthPx":8,"cellHeightPx":16},
  "captureHints":{"kind":"unavailable","userInitiated":true}
}
```

- Action is press/release/motion. Button is required nullable: left/middle/right/wheelUp/wheelDown/wheelLeft/wheelRight/back/forward/ten/eleven. Wheel steps use press + wheel button; fractional accumulation stays client-local. Preserve actual current anyButtonPressed and modifiers, not a pre-encoded protocol guess.
- Coordinates are content-local pixels, finite within +/-1e9, including legitimate outside drag/release. Cell pixel dimensions are positive bounded integers and the full canonical grid pixel extent must fit the existing encoder bounds. They describe presentation metrics, not canonical resize permission.
- Service resolves wire recovery UUID and ControlRef; Runtime `MouseStamped` checks the accepted stamp against the same current controller epoch AND current canonical revision/cols/rows before encoding. Invalid/stale/pending geometry produces `staleLayout` with `recoveryRequired:true`, no write, no clamping/retargeting/automatic retry. Add the matching native stale-layout error only with this handler.
- The sole Ghostty owner selects actual tracking mode/SGR/UTF-8/pixel encoding. Unsupported buttons or ignored tracking return the actual zero-byte receipt. Never route mouse through a high-byte string or accept client-generated escape sequences as StructuredMouse.

## Takeover, control loss and revocation

1. Read-only attachment owns only an observer. Successful take-control Claim produces a NEW controller epoch, retires the old observer/ref, and returns a complete new attachment/recovery plus separate ControlRef. Do not mutate the existing ref into a controller.
2. Claim blocked without takeover leaves that observer unchanged. A takeover requires explicit request plus current takeover/write authority. No silent takeover from attach, reconnect or resize.
3. When actual control is replaced/lost, invalidate BOTH old ControlRef and controller DeliveryRef before exposing more queued frames. Discard their buffered data/layout/recovery/replies that would grant new capability; surface a correlated already-accepted input outcome without implying continuing control. No hidden observer continuation and no manufactured ACK to keep bytes flowing.
4. A caller wanting read-only output after loss must explicitly reattach, receive a NEW real observer lease/ref and consume a NEW full recovery. Promotion likewise requires a new controller delivery/recovery.
5. Generation, approval, policy or corresponding permission revocation invalidates refs locally FIRST, then queues native Unobserve/ReleaseControl and retains/retries bounded cleanup work. A full native command queue is not permission to keep forwarding. Release failure never triggers process kill. Recheck identity/policy/permissions and actual-lease routing at outbound handoff, including already queued Runtime events. Stale events are fenced by their real lease, not just generation.
6. Service source event dispatch observes ControllerRevoked before polling old controller mailboxes. All claims/releases pass through the same source owner. If an additional direct Runtime claimant is introduced, it must supply the same ordered revocation/invalidation guarantee; a stale Service-only boolean is insufficient.
7. Metadata filtering remains a separate prior check. Control/delivery grant, eligibility, pending recovery and revocation events must not leak otherwise invisible tab/session IDs.

Manager-only revoked reasons add `controlLost`, `controlReleased`, `generationChanged` and `permissionChanged` alongside existing approval/policy/source reasons. Layout events retain delivery/ref, revision, dimensions and recoveryRequired=true; they also carry `lastSequence` for the exact ordered byte boundary in this manager schema. No event creates control authority.

## Required gates before negotiation is advertised

- Real controller path allocates NO observer; blocked path has NO controller ACK/credit effects. ACK dispatch matches the actual private enum variant and actual epoch.
- Stamped controller recovery is atomic; wrong/latest-replacement/duplicate/foreign IDs, data-ACK bypass, legacy-recovery downgrade, resize races, journal overrun and queue backpressure remain blocked. V1 and observer-v2 regression suites unchanged.
- Local takeover, remote takeover, disconnect/release, lost Claim reply and control permission loss retire old refs before another public data batch. No auto observer continuation/process kill. New delivery requires new recovery consumption.
- Binary 0x80/0xff/NUL bytes survive exactly; oversize/invalid base64 reject before write; correlated receipt follows actual complete I/O; duplicate/timeout/partial-write paths never resend. Capture null/stale/unavailable semantics do not recapture or inherit legacy unsettle behavior.
- Mouse uses real event fields and accepted geometry; stale epoch/recovery/layout and nonfinite/oversize metrics reject before encoding. Tracking-off zero output, wheel, outside release and SGR/pixel/high-byte encodings are tested through owner bytes, not strings.
- Actual administrator inheritance and Unknown + eligible deny; actual Normal + no grant denies; actual Normal + explicit grant/current permissions allows. Mapping-only fixture facts are never called native elevated/normal acceptance. Unsupported OS remains gated.
- Metadata permission denial is tested BEFORE IDs appear in catalog/attach/errors/revocation. Write/control grants do not imply metadata visibility. No new UI/default sharing.

Implementation ownership after this review: Runtime owner handles additive stamping/release/revocation/mouse guards; protocol owner defines separate manager structs/profile; Service owner installs real-lease association, policy/permission checks and input correlation. Cross-platform clients adopt only the negotiated schema, never platform-specific string encodings.
