# Manager-v2 concrete contract delta

Status: proposed for Root review, not implemented. The current observer-v2 input gate, strict DTOs/goldens and immutable 860d snapshot are unchanged. General interactive v1 remains the current full-control path. No new paths, Go transport API, native trust verbs or dependencies are proposed.

## 1. Profile and carrier

Use the SAME five POST /v2/discover, /v2/pair, /v2/request, /v2/events and /v2/disconnect paths and the SAME exact 43 verb strings. Add a separate manager-v2 typed model/client; do not widen observer-v2's Profile/Binding/Attachment/Request acceptance.

Discovery's existing shape is unchanged. Its bounded profiles list may advertise manager-v2 only when that profile is operational. Add no manager-only fields to the common discovery response.

Manager PairOffer is the existing v2 offer with profile set to manager-v2 and one additional REQUIRED selected field inputFormat:manager-input-v1. Manager PairSelection echoes that selection and adds the existing server connection ID. It also reports devicePermission:read-only|write, derived from the current host approval. This report is not itself source authority; every operation rechecks the actual approval and policy epoch.

```json
{"version":2,"clientId":"20000000-0000-4000-8000-000000000001","attemptId":"30000000-0000-4000-8000-000000000001","ownerId":"10000000-0000-4000-8000-000000000001","profile":"manager-v2","catalogMode":"complete","deliveryFormat":"vt-bytes-base64-v1","recoveryFormat":"formatted-active-buffer-v1","inputFormat":"manager-input-v1"}
```

All manager bound envelopes use a manager-only Binding with clientId, ownerId, connectionId and profile:manager-v2. The server binds actual authenticated peer plus this exact tuple. No JSON permission/profile flag creates a controller.

A general interactive factory may select usable manager-v2 or explicitly offer legacy v1 using only the existing validated PRE-PAIR fallback. It must never substitute observer-v2 as its interactive fallback. A genuine host ReadOnly approval is reported as such; provisional source restrictions must not be relabeled as device ReadOnly.

## 2. Exact references and Runtime association

```text
ControlRef = {tabId:UUID, generation:UUID, controlId:UUID, controlEpoch:positiveSafeInteger}
LayoutRef  = {tabId:UUID, generation:UUID, layoutId:UUID, layoutEpoch:positiveSafeInteger}
```

DeliveryRef is the existing v2 data-delivery reference. ControlRef and LayoutRef are separate manager wire capabilities, bound server-side to the same actual controller lease and connection. They cannot be reconstructed from DeliveryRef, Write permission, profile strings or source-kind tags.

| Actual result | DeliveryRef server mapping | ControlRef |
| --- | --- | --- |
| Claim granted | Actual Runtime controller delivery lease | Same actual Runtime ControllerLease |
| Claim busy or delivery-only request, with no controller already held by this view | Actual Runtime ObserverLease | null |
| Intrinsic raw/read-only view | Actual Runtime ObserverLease | null |

There is ONE delivery stream for that view. Do not create controller plus observer delivery for the same controlled view, and do not auto-ACK an unused controller stream. Controller data ACKs map through DeliveryRef to the real controller delivery; observer data ACKs map to the real observer delivery. An ACK never authorizes input.

## 3. Attach, claim and takeover

Manager attachTerminal payload:

```json
{"tabId":"50000000-0000-4000-8000-000000000001","resume":null,"control":"ifFree"}
```

control is REQUIRED and is exactly none|ifFree. none requests delivery without acquiring a new controller. ifFree checks actual device Write permission, trusted native-purpose eligibility/readiness and generation, then invokes real Claim(takeover:false). It never steals a busy controller.

Manager attachment replaces observer-v2's input/layout fields only in the NEW manager DTO:

```json
{"kind":"attached","delivery":{"tabId":"50000000-0000-4000-8000-000000000001","generation":"60000000-0000-4000-8000-000000000001","deliveryId":"70000000-0000-4000-8000-000000000001","deliveryEpoch":4},"input":{"tabId":"50000000-0000-4000-8000-000000000001","generation":"60000000-0000-4000-8000-000000000001","controlId":"a0000000-0000-4000-8000-000000000001","controlEpoch":9},"inputState":"owned","layout":{"tabId":"50000000-0000-4000-8000-000000000001","generation":"60000000-0000-4000-8000-000000000001","layoutId":"b0000000-0000-4000-8000-000000000001","layoutEpoch":2},"recovery":{"recoveryId":"80000000-0000-4000-8000-000000000001","format":"formatted-active-buffer-v1","encoding":"base64","data":"GBtj","lastSequence":12,"cols":80,"rows":24,"layoutRevision":3}}
```

This is an illustrative DTO, not a captured Runtime response. All IDs/epochs/stamps in production must be actual server records.

inputState is exactly owned|busy|notReady|deviceDenied|intrinsicReadOnly|sourcePolicyDenied. owned requires a real input ControlRef. Every other state requires input:null and layout:null. A source-unavailable attachment retains the existing typed unavailable form. No fake lease is used for busy or raw sources.

Explicit takeTerminalControl payload:

```text
{delivery:DeliveryRef, mode:"takeover"}
```

After current permission/eligibility checks, invoke real Claim(takeover:true). Return a fresh manager attachment with the resulting references and stamped recovery, not merely taken:true. Cancellation or lost reply is uncertain and never automatically repeats takeover.

An existing valid controller owned by this same view is not replaced gratuitously. Requesting none means no NEW acquisition: it retains and reports the existing real controller association when this view already owns it, rather than adding a duplicate observer stream. Explicit detach/disconnect handles release.

## 4. Stamped controller recovery and geometry gate

Add a controller-stamped recovery operation analogous to the approved ObserverRecoverStamped operation. It must capture bytes, actual capability, last delivery sequence, actual cols/rows and actual canonical layout revision at ONE barrier. Existing ANSI/v1 operations stay unchanged. FormattedActiveBufferV1 is still not FullState.

While that recovery/layout stamp is pending, hold later delivery and do not accept manager input/resize/upload side effects. The existing recovery ACK shape is reused through the new manager binding and DeliveryRef:

```json
{"delivery":{"tabId":"50000000-0000-4000-8000-000000000001","generation":"60000000-0000-4000-8000-000000000001","deliveryId":"70000000-0000-4000-8000-000000000001","deliveryEpoch":4},"ack":{"kind":"recovery","recoveryId":"80000000-0000-4000-8000-000000000001","sequence":12}}
```

The backend resolves recoveryId to its exact stored lease/generation/boundary/grid/revision stamp. Only this validated recovery ACK maps to controller layout acceptance. There is no standalone wire layout ACK or grant created by ACK. Native consumption of the complete recovery at its exact grid is required before sending it.

PTY resize payload is explicitly:

```text
{control:ControlRef, layout:LayoutRef, cols:integer, rows:integer}
```

Check both references, actual controller lease, native source resize support and current ready geometry gate. Apply the original interactive channel's resize, not observer ResizeExternal. A canonical grid change establishes a new stamped recovery/layout barrier before more delivery/input; revision increments only for actual grid change. No-op frame reads, byte counters or repeated identical sizes do not manufacture revisions.

## 5. Binary input, typed mouse and frozen hints

Manager writeTerminal payload:

```text
{
  control:ControlRef,
  inputId:UUID,
  hints:{userInitiated:boolean, activityHint:boolean, settledAt?:null|UtcTimestamp},
  input:
    {kind:"key", event:KeyEvent}
    | {kind:"paste", text:Utf8Text}
    | {kind:"bytes", encoding:"base64", data:CanonicalBase64, origin:"key"|"paste"}
    | {kind:"mouse", event:MouseEvent, geometry:GeometryStamp}
}
```

The same inputId binds the original pending input context to the actual Runtime request and returned receipt. It is correlation, not authority or a promise of persistent exactly-once replay. Duplicate pending IDs reject; uncertain input is never automatically resubmitted.

```json
{"control":{"tabId":"50000000-0000-4000-8000-000000000001","generation":"60000000-0000-4000-8000-000000000001","controlId":"a0000000-0000-4000-8000-000000000001","controlEpoch":9},"inputId":"c0000000-0000-4000-8000-000000000001","hints":{"userInitiated":true,"activityHint":true,"settledAt":null},"input":{"kind":"bytes","encoding":"base64","data":"QQBC","origin":"key"}}
```

QQBC is exactly A-NUL-B, never a NUL-terminated string. Decoded byte input is 1..65536 bytes. Key text/paste retain a 65536-byte bound. The byte branch uses a new narrowly checked INTERACTIVE terminal-input admission path after ControlRef resolution. It does not widen/export local RawInput or automation raw stdin to the network.

KeyEvent preserves the existing wrapper's physical key, press/repeat/release, modifiers, consumed modifiers, optional committed text, unshifted codepoint and composing fields. Named/function/character key values remain a closed validated vocabulary, not arbitrary OS commands. General UI should prefer authoritative owner key/paste encoding rather than assume incomplete recovery restored every input mode.

MouseEvent preserves press/release/motion, nullable button, modifiers, finite content-local pixel x/y and anyButtonPressed. GeometryStamp carries the sampled layoutRevision, cols/rows and nonzero cellWidth/cellHeight. The owner rejects stale geometry and encodes mouse reports using its actual parser modes. No encoded-mouse string or origin:mouse byte variant exists. X10 high bytes are produced and written as bytes by the owner encoder, not transported as Unicode lookalikes.

Freeze ControlRef, hints and mouse geometry at FIFO admission, not dequeue. settledAt omission/null/value remains distinct. These hints are not permission, privilege measurements or accepted-byte claims. Host classification and activity/settlement updates use the matching ACTUAL InputReceipt and original captured context.

Write response:

```json
{"inputId":"c0000000-0000-4000-8000-000000000001","kind":"bytes","acceptedBytes":3,"disposition":"accepted"}
```

kind is the actual matched bytes|key|paste|mouse receipt kind. disposition is accepted|ignored. Ignored encoder output has acceptedBytes:0 and cannot fabricate activity. Accepted counts are correlated with the actual prepared/accepted bytes, never copied from requested length. Errors before acceptance contain no successful receipt.

## 6. Uploads, detach and control-loss privacy

All four manager terminal-upload payloads replace caller-supplied controlEpoch identity with REQUIRED ControlRef, retaining the existing file name, size, uploadId, offset and canonical chunk limits. Begin captures its source/control association. Every chunk/finish rechecks current approval, native readiness and the exact controller lease. No observer upload is enabled.

Control loss cancels pending writes/uploads and invalidates ControlRef/LayoutRef. Cancellation cleanup may still identify and remove the original upload after revocation, without granting input. Finish returns a path only; it never implicitly writes/pastes it. A later explicit path insertion uses a valid current control reference and the original frozen intent hints, not a newly captured settlement marker.

Manager detachTerminal carries delivery plus releaseControl:null|ControlRef. Releasing a control grant also retires any delivery mapped to that same Runtime controller lease. Disconnect retires all references for that connection, never kills the original source. Remaining observation requires a newly issued real observer delivery and recovery; there is no silent continuation under stale controller delivery.

Takeover, permission/policy change, source replacement or control loss purges old queued output according to privacy policy, revokes old refs and gates replacement delivery on fresh stamped recovery. Both generation and actual Runtime epoch are rechecked at dispatch/acceptance, including async waits. Input is not replayed during recovery/reconnect.

CloseTab/DeleteSession remain independent manager lifecycle operations. Service must stop/drain the actual original source before runtime/metadata removal. Delivery, control and source-kind metadata alone are not cleanup proof.

## 7. Native-purpose eligibility under FullHostAccount

FullHostAccount is the proposed HOST-LOCAL user policy, not a deserializable remote permission claim. Device Write means the peer may request/take eligible control; it does not call Claim by itself.

Replace the provisional catalog-only eligibility capture with trusted, non-wire origin records minted by reviewed native owners:

| Origin | Required native evidence before Claim |
| --- | --- |
| Standard interactive PTY | Original reviewed catalog factory, exact generation, native Ready, eligible initial host launch |
| WSL interactive PTY | Original reviewed WSL factory and exact guest/owner launch receipt, generation and native Ready |
| SSH interactive PTY | Exact native SSH reservation/connection/runtime, authenticated Ready and non-canceled connection |
| Raw automation/external display | Intrinsic inputNone; no Claim, regardless of device Write |
| Unreviewed command origin | Source eligibility unavailable; do not label the device ViewOnly |

Current native_sharing excludes WSL and command-based origins. These are provisional SOURCE gates, not the user's device permission policy. SSH's existing control_admission already allows another permitted viewer to Claim its exact authenticated Ready runtime without adopting credential-management authority. Reuse that gate rather than bypass authentication or expose raw SshSession/stdin.

Host-launch privilege and guest privilege are different. SSH/WSL guest privilege stays Unknown until independently measured; it is never rewritten Normal because input is permitted. Under FullHostAccount, an already-authorized interactive guest channel may be eligible after its native readiness/authentication gate. No runas, alternate-token, deliberate elevated-launch or known-elevated-host override is added. Missing host launch proof is a source eligibility issue, not a device downgrade. Admin/elevation exclusions remain unchanged pending the user's explicit answer, including clarification for independently measured elevated guest contexts.

## 8. Review and implementation gates

Approve this concrete delta before protocol/core/backend/mobile implementation. Keep manager-v2 in a new model/client path; reuse only compatible presence-preserving v2 leaf DTOs and transport utilities. Observer-v2 shapes/goldens and ABI snapshot 860d are untouched. No new transport paths, 43 verbs, native registration/claim/completion wire APIs or dependencies are introduced.

Required acceptance: real Claim/Takeover association, no duplicate deliveries or phantom ACKs, busy/intrinsic inputNone, exact connection/generation/epoch/policy binding, stamped controller recovery and geometry gate, source-stop/drain lifecycle, binary NUL/high bytes, structured X10/SGR mouse, frozen hint boundaries, matched accepted-byte receipts, upload cancellation, privacy purge/control loss and no automatic retry. Test SSH pending/Ready/revoked and WSL provenance without claiming guest privilege measurement. New native creator/ABI and matching artifacts are separate coordinated work; never relabel the immutable observer snapshot.
