# Manager-v2 concrete JSON delta for review

Status: historical review input, superseded by Russell's approved manager-v2 contract and its reviewed amendments. Do not implement these sample DTOs as a competing schema. In particular, the approved contract has a separate LayoutRef, fixed approved-device policy, owner key/paste/mouse encoding and CapturedUnavailable settlement omission. Consume only Russell's compiled canonical exports and fixtures. Observer-v2 and all V1 fixtures stay unchanged. Device approval is always control + FullAccount, but that authority cannot fabricate a protocol controller lease.

## Carrier and negotiation

Use the same five exact POST /v2 paths and 43 method strings. Propose a separately selected profile literal `manager-v2`; do not broaden `observer-v2` decoding. Discovery may advertise the new profile only after its server/client/native artifacts pass their gates. Pair echoes the selected profile and the existing complete catalog/delivery/recovery choices. Binding keeps exactly clientId, ownerId, connectionId and selected profile. Replies use the existing canonical version-2 bound envelope.

## Two wire references, one actual lease association

DeliveryRef retains the existing fields:

```json
{"tabId":"60000000-0000-4000-8000-000000000001","generation":"70000000-0000-4000-8000-000000000001","deliveryId":"80000000-0000-4000-8000-000000000001","deliveryEpoch":4}
```

New ControlRef is a distinct validated DTO:

```json
{"tabId":"60000000-0000-4000-8000-000000000001","generation":"70000000-0000-4000-8000-000000000001","controlId":"90000000-0000-4000-8000-000000000001","controlEpoch":4}
```

Server-only association:

- Claimed: DeliveryRef and ControlRef both map to the SAME real Runtime ControllerLease, source and authenticated connection. Data/ACK uses controller delivery, with no second observer subscription.
- Blocked or intrinsic read-only: DeliveryRef maps to a real Runtime ObserverLease. No ControlRef exists.
- UUIDs identify current server records. They do not replace Actor permission, native-purpose policy, exact generation or Runtime epoch checks.

## AttachTerminal and TakeTerminalControl

Attach request remains `{tabId,resume:null|{generation,afterSequence}}`, but its result has a separately validated manager-v2 control union. Example controlled result inside BoundReply.value:

```json
{
  "kind":"attached",
  "delivery":{"tabId":"60000000-0000-4000-8000-000000000001","generation":"70000000-0000-4000-8000-000000000001","deliveryId":"80000000-0000-4000-8000-000000000001","deliveryEpoch":4},
  "control":{"kind":"owned","ref":{"tabId":"60000000-0000-4000-8000-000000000001","generation":"70000000-0000-4000-8000-000000000001","controlId":"90000000-0000-4000-8000-000000000001","controlEpoch":4}},
  "layout":{"kind":"ptyController"},
  "recovery":{"recoveryId":"a0000000-0000-4000-8000-000000000001","format":"formatted-active-buffer-v1","encoding":"base64","data":"GBtj","lastSequence":12,"cols":80,"rows":24,"layoutRevision":2}
}
```

Observed result uses the same attached branch and real observer delivery, with one of these control states and fixedCanonicalGrid layout:

```json
{"kind":"blocked","reason":"anotherController"}
```

```json
{"kind":"unavailable","reason":"intrinsicReadOnly"}
```

Other closed unavailable reasons: authPending, sourceUnavailable, policyDenied. Authorization failure remains an error, not successful unavailable. Raw presentation is intrinsicReadOnly, never a device ViewOnly role.

Attach uses real Claim without takeover when native purpose permits control. A held controller cannot be displaced merely because the device has Write permission. TakeTerminalControl keeps its tabId/generation input but returns this complete attachment result after the explicit real takeover. Claim failure may create real observer delivery only under the reviewed observation policy.

Takeover/control loss first revokes BOTH old refs, invalidates queued output/recovery/input authority, then returns or emits the new attachment under its real epoch and full recovery. Do not silently keep old controller delivery as observation. Proposed manager-only notification:

```json
{"type":"controlLost","delivery":{"tabId":"60000000-0000-4000-8000-000000000001","generation":"70000000-0000-4000-8000-000000000001","deliveryId":"80000000-0000-4000-8000-000000000001","deliveryEpoch":4},"control":{"tabId":"60000000-0000-4000-8000-000000000001","generation":"70000000-0000-4000-8000-000000000001","controlId":"90000000-0000-4000-8000-000000000001","controlEpoch":4},"reason":"takenOver","lastSequence":12}
```

Closed reasons: takenOver, sourceReplaced, nativePurposeRevoked, approvalRevoked, connectionReplaced. The event revokes existing references; it cannot carry new input authority. A fresh observed/controlled attachment requires a separately correlated response and full recovery.

## Binary WriteTerminal and correlated receipt

New manager-v2 payload, no high-byte string conversion:

```json
{
  "control":{"tabId":"60000000-0000-4000-8000-000000000001","generation":"70000000-0000-4000-8000-000000000001","controlId":"90000000-0000-4000-8000-000000000001","controlEpoch":4},
  "inputId":"b0000000-0000-4000-8000-000000000001",
  "input":{"kind":"bytes","encoding":"base64","data":"eAo="},
  "context":{"userInitiated":true,"hint":true,"settledAt":null}
}
```

context and settledAt are REQUIRED. settledAt is nullable UTC timestamp, captured at native bounded-outbox admission. Missing context cannot become captured null or Legacy. Bytes are canonical RFC4648 base64, decoded length 1..65536, additionally bounded by Runtime's actual input limit.

Successful value is correlated to the exact request and actual receipt:

```json
{"inputId":"b0000000-0000-4000-8000-000000000001","control":{"tabId":"60000000-0000-4000-8000-000000000001","generation":"70000000-0000-4000-8000-000000000001","controlId":"90000000-0000-4000-8000-000000000001","controlEpoch":4},"kind":"bytes","acceptedBytes":2,"metadata":{"state":"committed","resumed":false}}
```

metadata.state may be deferred. Persistence failure never turns a positive accepted receipt into a failed/retriable write. acceptedBytes means full encoded OS-write acceptance, not read/executed. A native encoder's zero-byte mouse result is acceptedBytes:0 with no OS write, echo suppression or settlement/TTL input effect.

No automatic retry after timeout/cancel/uncertain acceptance. Reusing inputId must never execute bytes again. A bounded duplicate-id tombstone can return a nonretryable uncertain-outcome error; it must not retain raw input/password bytes. Parser replies do not acquire input IDs, user hints, receipts or CAS effects.

## Structured mouse on the same WriteTerminal verb

The input union additionally accepts:

```json
{
  "kind":"mouse",
  "event":{"action":"press","button":"left","modifiers":{"shift":false,"alt":false,"control":false,"super":false},"x":120.0,"y":48.0,"anyButtonPressed":true},
  "geometry":{"layoutRevision":2,"cols":80,"rows":24,"cellWidth":8,"cellHeight":16,"presentationSequence":12}
}
```

Closed actions: press, release, motion. button is required nullable and uses the actual native button names left/middle/right/wheelUp/wheelDown/wheelLeft/wheelRight/back/forward/ten/eleven. Wheel steps are press events. x/y are finite content-local pixels within +/-1e9, including outside release/drag positions. Cell sizes and multiplied screen dimensions must satisfy the native encoder bounds.

Geometry must come from the exact accepted native view/cache, not an arbitrary JS rectangle. The owner checks current source generation/layout and the reviewed presentation-sequence fence, then passes the actual typed MouseEvent/MouseGeometry to the ORIGINAL Runtime encoder. It uses current source mouse modes, never a client-built escape or lossy string. This geometry is encoder context, not a source resize grant. Exact presentation-sequence acceptance versus bounded source-ahead handling still needs review.

## ACK, resize and upload

AcknowledgeTerminal keeps DeliveryRef plus recovery/data ACK. Controller association maps to real controller ACK; observer association maps to observer ACK. Recovery UUID maps to the exact opaque Runtime stamp. No controller+observer duplicate ACK paths.

ResizeTerminal payload becomes `{control,cols,rows,expectedLayoutRevision}` with native grid bounds. Only real controller purpose may resize PTY. It returns actual canonical revision/geometry, and ordered layout notification blocks later bytes until the matching full stamped recovery is consumed. No observer ResizeExternal shortcut.

Terminal-upload methods require ControlRef and current native interactive-purpose permission on every begin/chunk/finish/commit. File transfer methods remain separate host-account filesystem operations with unchanged device policy.

## Additive Runtime requirement

Propose ControllerRecoverStamped {lease,capability} returning Recovery bytes, lastDeliverySequence, actual cols/rows, canonical layoutRevision and opaque RecoveryId at ONE barrier. Add exact ControllerLayoutAck {lease,recoveryId,lastDeliverySequence,layoutRevision}, plus ordered ControllerLayout notification and no later controller bytes until full recovery acceptance. Existing Ansi/V1 behavior stays untouched.

The stamp is bound to exact latest snapshot + real controller lease + source/layout, not only sequence/revision. A stale same-sequence ACK cannot unlock replacement. Runtime/controller loss revokes the association before further bytes/input, with retained cleanup custody.

## Gates and unresolved facts

formatted-active-buffer-v1 restores bounded active history, supported extras and exact continuation. It is NOT FullState, inactive-buffer or saved-cursor restoration. Before default interactive migration, validate every mode required by the client-native keyboard/paste encoder or separately specify truthful input-mode synchronization/owner encoding. Raw binary transport alone does not prove mode parity.

SSH/WSL native-purpose eligibility and guest privilege facts remain the separately proposed policy review. Local token Normal cannot turn guest privilege Normal. Admin/elevated boundaries stay held. No source kind, grant or SourceEnded event proves original process cleanup.
