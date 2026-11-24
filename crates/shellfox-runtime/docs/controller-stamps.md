# Controller stamps

How the Runtime gates a controlling client's input on the layout it has actually seen. This is Runtime behavior, not a wire schema or permission policy. V1 and Observer APIs behave as before, and the Runtime doesn't depend on the Service or protocol crates.

## Public API

```rust
Command::RecoverStamped { lease: Lease, capability: AnsiRecoveryCapability }
    -> Response::Recovery(StampedRecovery)
Command::ControllerLayoutAck {
    lease: Lease,
    recovery_id: RecoveryId,
    last_delivery_sequence: u64,
    layout_revision: u64,
} -> Response::Applied
Command::InteractiveBytes { lease: Lease, bytes: Vec<u8> }
    -> Response::Input(InputReceipt) // kind: InputKind::Raw
Command::CheckControllerReady { lease: Lease } -> Response::Applied
Command::MouseStamped {
    lease: Lease,
    event: shellfox_ghostty_vt::MouseEvent,
    geometry: shellfox_ghostty_vt::MouseGeometry,
    stamp: ControllerGeometryStamp,
} -> Response::Input(InputReceipt)
Command::ReleaseControl { lease: Lease } -> Response::Applied

struct ControllerGeometryStamp {
    pub recovery_id: RecoveryId,
    pub layout_revision: u64,
    pub cols: u16,
    pub rows: u16,
}
Event::ControllerLayout { lease: Lease, layout: LayoutStamp }
Event::ControllerRevoked { lease: Lease, reason: ControllerRevocation }
enum ControllerRevocation { Replaced, Released }
```

Existing `StampedRecovery` carries bytes, actual capability, last_delivery_sequence, cols, rows, layout_revision and opaque RecoveryId. Existing `LayoutStamp` carries actual cols/rows/revision and the preceding byte boundary. RecoveryId remains opaque, Copy/Eq and non-Serde. It now includes a private Runtime domain so IDs from independent Runtime instances cannot alias.

`MAX_INPUT_PAYLOAD_BYTES` and `MAX_ENCODED_INPUT_BYTES` are distinct public names, both currently 65536. `MAX_INPUT_BYTES` remains the payload compatibility alias. Encoding overflow in a stamped epoch returns `ErrorCode::ResourceLimit` before any prewrite hook or write; ordinary legacy epochs retain their original validation behavior. Input is never split or truncated, and there are no partial receipts. Full-size bracketed paste and expanded key input are not supported yet.

`ErrorCode::RecoveryRequired` means the real epoch has not consumed its latest full recovery/current canonical layout. `ErrorCode::StaleLayout` rejects a mismatched mouse recovery ID/revision/grid. Neither is a fatal PTY error. No input event is clamped, retargeted or automatically retried.

## Epoch and delivery rules

- Use the actual `Lease` returned by Claim. No separate input lease, grant or parallel observer is created. RecoverStamped opts THAT epoch into stamped gating only after serialization succeeds.
- Snapshot bytes, sequence and real canonical geometry are captured in one owner-thread barrier. Recovery replaces real credit, but does not ACK itself, including at sequence zero.
- ControllerLayoutAck must match the latest opaque ID, exact sequence and revision. It validates before changing real delivery credit. Duplicate, superseded, observer, foreign Runtime and old-epoch stamps cannot consume the fence.
- Ordinary Ack cannot consume a pending stamped recovery. Legacy Recover/RecoverAnsi/Replay cannot downgrade an opted-in epoch. A fresh never-opted V1 epoch retains legacy behavior.
- ControllerLayout is ordered after old-grid bytes and before new-grid bytes. Once a layout barrier is reached, later output and end-of-stream wait for a new full recovery. Queue-full notice/data publication advances no credit.
- Reuse the one canonical 64-change journal and the one bounded byte replay bank. Capacity is at most one controller fence plus 16 observer fences. Resize only increments revision when actual dimensions change. No synthetic delivery sequences or geometry claims from frame versions.
- Resize after a sampled snapshot still leaves a current-layout barrier even if that older snapshot is accepted. Journal/replay loss requires fresh recovery, not a retained-tail skip.

## Side effects and input

InteractiveBytes and CheckControllerReady require an opted-in, consumed, current-layout epoch. Existing Key/Paste/RawInput/Mouse/Resize/Colors also check the gate whenever their real epoch opted into stamping. Their ordinary V1 epochs are unaffected. Parser-generated replies remain independent of user-input barriers and receipts.

CheckControllerReady performs the current owner-thread lease/layout/active check for a Service-controlled side effect outside Runtime, such as each upload begin/chunk/finish. Applied is an observation, not a reusable permission token. The code performing that side effect must still check current approval, policy, source identity, association and revocation when it commits. Runtime performs no filesystem uploads and issues no ShareGrant.

InteractiveBytes uses the actual bounded PTY byte writer without exposing the local RawInput API as a wire capability. It returns the existing mechanical InputKind::Raw receipt. Typed key/paste commands continue to let the sole Ghostty owner encode current modes. UI origin, capture hints and CapturedSettlement::Unavailable stay outside Runtime, recorded by the Service when it admits the input. There is no marker resample or legacy fallback.

MouseStamped requires the last accepted opaque recovery ID and the sampled canonical grid/revision. The Service adapter carries that server-owned native ID alongside the original admitted geometry; it must not substitute a newer ID on dequeue. Cell pixel metrics and finite content-local coordinates retain the wrapper's existing bounds. Ghostty selects actual mouse protocol/modes. Ignored encoding returns Input(0), with no prewrite hook or PTY write.

The matching RequestId and positive InputReceipt confirm complete encoded PTY write, not program read/execution. The controlled binary fixture verifies NUL/high bytes at the prewrite boundary and the real completed write receipt. It does not claim ConPTY preserves arbitrary malformed UTF-8 as target-console input. Input/password bytes do not appear in production receipts.

## Release and replacement

ReleaseControl checks and retires only the actual lease. It clears its delivery credit/fence/presentation and emits ControllerRevoked Released without PTY shutdown, parser reset, source removal or observer adoption. A subsequent Claim receives a fresh real epoch. Successful Claim replacement emits ControllerRevoked Replaced before its reply and before any new-epoch delivery. Failed Claim does not revoke the existing lease.

Under event-queue backpressure, revocations and replies stay in order and are not dropped. The caller must invalidate its old wire refs/mailbox locally before forwarding queued events and retain/retry bounded lease retirement as needed. Runtime cannot retract an old event already handed to the caller. Continued observation after control loss requires an explicit new actual observer and full recovery.

## Tests

- Unit tests cover atomic stamps, wrong, latest and foreign IDs, scalar and downgrade rejection, pending side effects, the bounded journal and backpressure, stale geometry, and release and replacement ordering.
- PTY tests cover source output during barriers, exact binary bytes at the prewrite hook and the write receipt, canonical resize and upload-readiness checks, takeover and release without killing the process, X10 high-byte and SGR mouse reports, and rejection of bracketed paste and Kitty input at the size limit before any write.
- The mouse test child turns on its own Windows VT input and output modes before it enables mouse tracking. Without that, ConPTY consumes the request and tracking stays off. Production console defaults are unchanged.

The PTY tests have run natively only on Windows. The cross-platform tests exercise the shared handlers, not native Linux or macOS behavior or elevated sharing.
