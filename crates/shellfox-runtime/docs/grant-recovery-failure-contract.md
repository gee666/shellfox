# Grant and stamped-recovery failure contract

Scoped producer audit for Walter's bounded recovery review. No Runtime API or ledger rewrite, Service/protocol/sys edit, Service-lib relink or transport action is part of this audit. Statements below concern one exactly correlated native command attempt in one Runtime, not a reused RequestId or an RPC timeout.

## Ordinary Reply::Err is not transport uncertainty

An actual `Event::Reply { id, result: Err(error) }` means the owner executed that identified command and returned an ordinary error. A missing reply, caller cancellation, panic, event/transport disconnect, timeout or unobserved late success is NOT that result. Do not manufacture a lease, infer the next epoch, or record no-grant because a reply was not observed. RequestId provides correlation, not replay deduplication.

The native owner serializes command handling, with no await/reentrant Service call inside these grant/recovery handlers. The late Result-returning bookkeeping calls described below have already-validated lease membership and cannot ordinarily fail without breaking that single-owner invariant.

The no-new-grant guarantee is specific to Claim/Observe and this exact attempt. It is not a global rollback promise for every Runtime command/error. Source output may progress independently; another queued command may replace the source/lease after this result's linearization point.

## Claim

Order in `commands.rs` and `delivery.rs`:

1. ExternalBytes returns Unsupported before calling Delivery.claim.
2. Delivery.claim rejects a different current controller without takeover.
3. It checks epoch increment before mutation, including same-controller refresh and takeover.
4. Only then does it store the new exact controller Lease, reset that controller's initialized/credit/recovery fields and advance epoch.
5. Remaining handler work removes the previous controller layout fence, queues its ordered revocation, prunes presentation state, clears the legacy frame and returns Claimed. These operations have no ordinary Result error path.

Therefore an actual Claim Reply::Err creates no new controller grant, consumes no epoch and emits no replacement revocation. A refused busy Claim does not erase the old accepted layout/readiness or its real lease. Allocation panic/OOM/thread death is outside this ordinary-error guarantee.

On success, the actual new lease persists regardless of whether the caller receives or retains Claimed. The prior lease is fenced; the new delivery starts uninitialized and needs explicit replay/recovery. Merely dropping the successful response does not release it. Retrying Claim is a NEW grant operation: the same controller can refresh to another epoch, and takeover can replace another controller. It is not a read-only lookup or proof that the earlier operation created no grant.

## Observe

Order in `observers.rs` and `observer_commands.rs`:

1. Look up any existing slot for the viewer.
2. Reject a new viewer at 16 slots, then reject a supplied cursor ahead of the journal.
3. Check the observer epoch increment.
4. Build and replace/append the View and commit epoch.
5. Prune same-viewer stale presentation/layout state and return Observed. Subsequent handler steps have no ordinary Result error path.

Therefore an actual Observe Reply::Err creates no new observer, consumes no observer epoch and does not replace the old same-viewer lease, credit or pending recovery. Capacity refusal cannot leave a hidden 17th observer. Existing low-level tests also cover counter exhaustion before mutation.

Important distinction: a cursor behind retained history is NOT an Observe error. Observe can return a real grant which subsequently reports ReplayLost and requires recovery. Do not classify that success/event combination as no-grant.

On success, the actual observer persists if its reply is lost. `after:None` leaves it uninitialized; `Some(after)` can permit actual delivery after the response is queued, or a ReplayLost notice for an expired cursor. Repeating Observe for that viewer replaces it with a NEW actual epoch; it is not an idempotent query. A failed reattachment leaves the old grant intact.

Observer epoch exhaustion currently returns Unavailable. The owner drive may then run its existing source failure/cleanup path before queuing the error reply. No NEW observer was created, but this is not a promise that process phase or preexisting delivery availability stayed unchanged. Claim epoch exhaustion maps to ControlRequired. Neither case authorizes cleanup/ownership conclusions from an absent reply.

## RecoverStamped and ObserverRecoverStamped ordinary errors

| Stage | Controller | Observer |
| --- | --- | --- |
| Initial membership | Owner.command checks the exact current controller Lease. | Observer command checks the exact ObserverLease. |
| Serialization | recovery_bytes builds the requested bounded bytes before layout or credit replacement. | Same formatter before layout or observer credit replacement. |
| Stamp preparation | Layout checks capacity/domain/counter before storing the new token. | Same checks; normal owner observer-slot invariants already bound capacity. |
| Commit | Store opaque pending stamp, then Delivery.recover for the previously checked same lease. | Store opaque pending stamp, then ObserverHub.recover for the previously checked same observer. |
| Success | Return actual bytes/capability/sequence/cols/rows/revision/opaque ID. | Same, without changing the controller. |

Invalid/stale membership, serialization refusal, and stamp capacity/domain/ID exhaustion are ordinary pre-replacement errors. They do not issue a new lease or token, reset credit, opt a previously unstamped controller epoch into stamping, or supersede an existing pending token. A failed replacement leaves its prior exact token valid under its original conditions. Formatting can fail while the parser remains live, such as exhausted unfinished-continuation tracking; the command maps that optional formatter failure to Unsupported rather than pretending to have returned a new snapshot.

After stamp storage, the trailing Delivery.recover/ObserverHub.recover functions are Result-typed only because they check membership. Membership cannot change between the earlier check and that call in the current single-owner synchronous handler. There is no reachable ordinary late error requiring a rollback ledger. If unsafe corruption, panic or a future reentrant/asynchronous change breaks that invariant, do not classify its missing outcome as no-grant/no-recovery.

## Successful recovery response lost

These mutations occur BEFORE successful response publication and are NOT rolled back when its bytes/result are discarded:

| State | Controller RecoverStamped | ObserverRecoverStamped |
| --- | --- | --- |
| Lease | SAME actual controller and epoch | SAME actual observer and epoch |
| Opaque layout token | Latest pending ID + captured sequence/revision, needs_recovery=true | Same, for that observer only |
| Accepted prior token | May remain stored, but pending replacement blocks using it for new mouse/input. | May remain stored; pending replacement blocks delivery consumption. |
| Delivery cursor | sent becomes the actual current parser/journal sequence | observer sent becomes that actual sequence |
| Credit | Pending data-credit entries cleared; initialized=true; recovery_pending=Some(sequence) | Same for this observer; blocked/loss_reported cleared |
| ACK | Existing acked cursor is NOT advanced by recovery | Existing acked cursor is NOT advanced |
| Output/end | Held by the pending full-recovery/layout fence | Held for that observer |
| Input/resize/upload readiness | Held for that opted-in controller epoch | Observer never gains input; controller readiness is unchanged |

A native sent cursor equal to the snapshot boundary is NOT proof that the client received or consumed bytes. In particular sequence zero still has a real pending recovery barrier. Ordinary scalar data ACK cannot consume the latest stamped recovery. Layout notification itself does not grant resize or release a barrier.

Parser feeds and parser-generated replies continue while delivery waits. Earlier events already handed to the caller cannot be recalled; the caller must fence/purge superseded buffered delivery using its exact refs and lease. Phase/source cleanup observations remain separate from delivery consumption.

## Explicit fresh recovery after a lost success

A caller retaining the SAME known real current lease may explicitly request fresh stamped recovery while a previous recovery is pending. It need not ACK the lost snapshot first.

On fresh success:

- Capture current actual canonical bytes/sequence/grid/revision at a new owner barrier.
- Replace the pending token with a NEW opaque ID, even if sequence and revision are unchanged.
- Replace credit/cursor with the fresh snapshot boundary, still without advancing acked or opening output/input.
- Reject the earlier token. Only consumption of the NEW complete recovery followed by its exact ControllerLayoutAck/ObserverLayoutAck opens its barrier.

This is replacement recovery, not a fake ACK, raw-tail skip or new control grant. Earlier state is represented by the new actual snapshot. If fresh recovery fails ordinarily, the previous pending state remains; the failure does not make an unknown old snapshot consumable. If its result is also lost, uncertainty persists, with one latest pending stamp rather than an expanding native ledger. The caller still needs bounded admission/retry/retirement policy.

After an exact old snapshot is accepted, a newer canonical resize can still leave a layout barrier. Fresh recovery does not override process-active checks, current lease checks, policy/permissions or metadata authority. Native legacy ObserverRecover deliberately remains a separate legacy operation and can remove observer stamped tracking; manager consumers must keep using the approved stamped path, not use legacy recovery as an implicit bypass. Controller stamped epochs reject legacy downgrade.

## Focused regression evidence

Added without production API/state-transition changes:

- Actual owned-PTY Reply::Err regression for busy Claim, new/replacement ahead-cursor Observe, full observer capacity, old-token survival, actual old-controller write/readiness, no revocation and no consumed grant epochs.
- Ordinary formatter replacement failure preserves both prior controller/observer pending tokens and delivery cursors.
- Recovery domain/ID failure cannot replace a prior pending token or advance its counter.
- Dropped successful controller and observer recovery responses leave their exact real lease/barrier in place; explicit fresh recovery supersedes without ACK and only its actual consumption releases subsequent bytes.

Existing Delivery/ObserverHub counter-exhaustion and denial-credit tests remain relevant. Runtime all-targets passed 164 tests, comprising 103 unit and 61 integration tests; scoped all-target Clippy with warnings denied passed. This audit added four unit regressions and one actual owned-PTY regression, with no production transition/API change.

Leaf tests are controlled producer evidence, not RPC/Service recovery acceptance, OS capture acceptance or proof that a disconnected peer has no native grant.
