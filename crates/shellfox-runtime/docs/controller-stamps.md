# Compiled controller-stamp producer

Runtime implementation, not a wire schema or permission policy. Russell's canonical manager-v2 contract and reviewed amendments own JSON. V1 and Observer APIs keep their existing behavior. No Runtime → Service/protocol dependency is added.

Full physical keyboard input is now available through the additive [owned KeyV2 producer](key-v2-producer.md). It consumes the compiled safe value types without changing existing Key or declaring OS capture accepted.

## Exact public API

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

`MAX_INPUT_PAYLOAD_BYTES` and `MAX_ENCODED_INPUT_BYTES` are distinct public names, both currently 65536. `MAX_INPUT_BYTES` remains the payload compatibility alias. Encoding overflow in a stamped epoch returns `ErrorCode::ResourceLimit` before any prewrite hook or write; ordinary legacy epochs retain their original validation behavior. No splitting/truncation/partial receipt is added. Full-size bracketed paste and expanded key input remain an explicit parity gap. See [the vocabulary and budget audit](input-vocabulary-audit.md).

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

CheckControllerReady performs the current owner-thread lease/layout/active check for a Service-controlled side effect outside Runtime, such as each upload begin/chunk/finish. Applied is an observation, not a reusable permission token. The owner of that side effect must still enforce current approval, policy, source identity, actual association and revocation at commit. Runtime performs no filesystem uploads and issues no ShareGrant.

InteractiveBytes uses the actual bounded PTY byte writer without exposing the local RawInput API as a wire capability. It returns the existing mechanical InputKind::Raw receipt. Typed key/paste commands continue to let the sole Ghostty owner encode current modes. UI origin, capture hints and CapturedSettlement::Unavailable remain outside Runtime and must stay frozen in the existing Service admission book. No marker resample or legacy fallback is introduced here.

MouseStamped requires the last accepted opaque recovery ID and the sampled canonical grid/revision. The Service adapter carries that server-owned native ID alongside the original admitted geometry; it must not substitute a newer ID on dequeue. Cell pixel metrics and finite content-local coordinates retain the wrapper's existing bounds. Ghostty selects actual mouse protocol/modes. Ignored encoding returns Input(0), with no prewrite hook or PTY write.

The matching RequestId and positive InputReceipt confirm complete encoded PTY write, not program read/execution. The controlled binary fixture verifies NUL/high bytes at the prewrite boundary and the real completed write receipt. It does not claim ConPTY preserves arbitrary malformed UTF-8 as target-console input. Input/password bytes do not appear in production receipts.

## Release and custody

ReleaseControl checks and retires only the actual lease. It clears its delivery credit/fence/presentation and emits ControllerRevoked Released without PTY shutdown, parser reset, source removal or observer adoption. A subsequent Claim receives a fresh real epoch. Successful Claim replacement emits ControllerRevoked Replaced before its reply and before any new-epoch delivery. Failed Claim does not revoke the existing lease.

Event-queue backpressure retains ordered revocation and reply custody. The caller must invalidate its old wire refs/mailbox locally before forwarding queued events and retain/retry bounded lease retirement as needed. Runtime cannot retract an old event already handed to the caller. Continued observation after control loss requires an explicit new actual observer and full recovery.

## Checked on Windows

- Runtime all-target suite after owned KeyV2: 159 passed, comprising 99 unit and 60 integration tests.
- Scoped all-target Clippy with warnings denied passed. Existing vendored portable-pty warning is separate.
- Seven new unit tests cover atomic stamps, wrong/latest/foreign IDs, scalar/downgrade rejection, pending side effects, bounded journal/backpressure, stale geometry and real release/replacement ordering.
- Six new owned-PTY tests cover real source feeds during barriers, exact binary prewrite/actual receipt, canonical resize and upload-readiness checks, takeover/release without process kill, actual X10 high-byte/SGR mouse reports, bracketed-paste/Kitty boundary rejection before any write with exact positive receipt correlation, and owned full-vocabulary KeyV2 FIFO/receipt/stamp behavior.
- The controlled mouse child explicitly enables its own Windows VT input/output modes before emitting tracking modes. Without that fixture prerequisite ConPTY consumes the request and tracking stays off. No production console/factory defaults, output filtering or normal desktop relink changed.

Linux/macOS native acceptance remains unverified on this Windows host. Cross-platform owner tests exercise the shared handlers; they do not claim native platform acceptance or elevated sharing authority.
