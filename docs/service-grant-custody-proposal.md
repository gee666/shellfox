# Service grant-request custody API for review

Status: implementation checkpoint. Service metadata check and the separate injected-state integration target pass. No Service-lib test executable was relinked/run; this is not native controller acceptance.

## Verified existing behavior

`terminal_commands.rs::call_admitted` submits before setting the one transient `OwnedTerminal.waiting` slot. After 12 seconds or another error it clears that slot. The sole Service drain drops a later reply that no longer matches it. Claimed and Observed can therefore become real Runtime grants with no retained request outcome.

The Rust private transport already has a fresh 16-byte response trailer and echo in `local_connection.rs::response_consumed` and `local_client.rs::send`. It proves that the transport read/echoed bytes. The client echoes before shared-client response decoding, so it does **not** prove application acceptance or grant adoption. Native oneshot send success likewise does not prove recipient consumption. V1 HTTP and application DTOs gain no fields or ACKs from this work.

## Separate responsibilities

1. One bounded **Service grant-request ledger** owns unresolved Claim/Observe outcomes and exact-lease retirement. It is not a delivery mailbox.
2. A real current-binding lifetime/view book can explicitly adopt an actual grant. Server-side adoption does not claim that the client consumed terminal data.
3. Recovery bytes, opaque layout barriers, offered output and ACK custody remain separate. Do not combine their byte retention with controller/observer ownership receipts.

## Proposed private API

```rust
pub(crate) struct GrantTicket; // opaque request/identity/kind/nonce, not serde
pub(crate) struct ControllerGrantOffer; // actual Runtime Lease + ticket
pub(crate) struct ObserverGrantOffer;   // actual Runtime ObserverLease + ticket

with_grant_recipient(canonical_cancel, operation);
controller_grant(tab, controller, takeover)
    -> Result<ControllerGrantOffer, GrantFailure>;
observer_grant(tab, viewer) -> Result<ObserverGrantOffer, GrantFailure>;
adopt_controller_grant(offer, GrantHolder) -> Result<Attachment>;
adopt_observer_grant(offer, GrantHolder) -> Result<ObserverLease>;

// Existing owner tick; never requires another queue notification on drop.
pump_grant_retirements() -> Result<()>;
// Sole Runtime event drain, matching actual replies/revocations only.
record_ownership_reply(identity, event);
// Called only after original generation cleanup/destruction proof.
ownership_source_destroyed(identity);
```

`GrantFailure` distinguishes:

- `NotSent`, with typed pre-enqueue Invalid/Full/Disconnected reason. Reservation is cancelled only after proof that no request entered Runtime.
- `Rejected`, only for a matching **actual Runtime no-grant result** under an audited atomic grant contract. A generic Service error, ownership loss, timeout or lost reply is never rejection proof.
- `Uncertain`, with retained ticket and typed reason. The original request remains owned.

`GrantPoll` is Pending or Granted with the actual offer. Reserve before queue admission. Bind every entry to request ID, exact Runtime identity/generation, requested controller/viewer and recipient context. Bound entries at 128. While any sent Claim for an exact source/generation has no actual result, ALL new Claims for that source are rejected before queueing, including freshly allocated controller IDs. Once the actual GrantedHeld result is known, another authorized explicit takeover may proceed. Same-controller unresolved custody still blocks duplicates. Cleanup and describe/poll remain available. This NotSent/pending conflict is never M2 Busy-observer fallback; that fallback requires actual Runtime Rejected(ControlRequired). Tickets identify retained outcomes; callers never retry a new Claim to resolve uncertainty. Offers expose the actual lease and exact source identity. Dropping an offer or accepted wait writes a ledger-owned cancellation/abandonment cell directly, including while the owner queue is Full.

## Actual event handling

The existing sole Service drain captures matching actual grant replies by request ID before the transient waiting slot can drop them. Store the real Claimed/Observed lease, or the explicit no-grant result. Verify identity, requested controller/viewer and expected reply kind. Do not construct a lease from request fields.

The same ledger consumes matching release/unobserve completion and ControllerRevoked for its exact held lease. Unrelated native, input-book and activity replies still follow their existing routing. No second Runtime receiver or mailbox-local Claim ledger is added.

Scoped producer contract, statically verified in the current Runtime: `commands.rs` rejects unsupported Claim before delivery mutation. `Delivery::claim` checks busy/epoch exhaustion before assigning epoch/owner; remaining handler steps are infallible and return Claimed. `ObserverHub::observe` checks slot/cursor/epoch failures before replacing/pushing the view; remaining handler steps are infallible and return Observed. A matching actual Reply::Err for those exact commands therefore proves no NEW grant, not global state rollback: Observe Unavailable may trigger source cleanup, and expired history can succeed with a real lease then ReplayLost. The precise producer audit is `crates/shellfox-runtime/docs/grant-recovery-failure-contract.md`. Host errors, timeout, disconnect, panic, mismatched response kinds and unmatched replies do not. No English error classification is used. Future fallible work after grant mutation invalidates this contract and must change the producer result before being added.

## Attach and adoption contracts

`attach` and `attach_manager` share one Claim path. Legacy attach retains its existing signature/behavior and adopts a successful actual lease into its real Service controller/lifetime book. Manager IfFree still passes the requested takeover bit unchanged.

M2 receives a `ControllerGrantOffer`, not a fabricated lease. It may adopt only after it has installed that actual lease in a real current-binding view/mailbox custody book and revalidated the binding. Before that point the Service ledger owns it. A cancelled operation before adoption abandons the ticket and retains exact cleanup custody.

Native/legacy reply-delivery failure must preserve the server lifetime/view book or leave the grant in unresolved custody. A recipient cancellation signal is retained independently from the oneshot. It cannot erase a sent request. A send failure is not a Runtime rejection, and successful send is not a data ACK. Existing lifetime detach/EOF or explicit view disposal initiates exact-lease retirement. No new V1 fields, trailer ACK or synthetic consumption acknowledgment is introduced.

Adoption validates the exact real source/book/lease and holds the mutable terminal plus `MailboxAdoption` before transfer. The ledger checks the canonical recipient token at its adoption linearization point. After transfer, only infallible book `commit()` and field writes remain: the caller must not perform a separately fallible mark-adopted step. Provisional books purge data on revocation but do not locally release central-owned grants; `discard_unadopted` refuses adopted books. The destination book then owns normal lease retirement. Later request cancellation cannot release an already-adopted view lease. It must not be possible for an old offer-drop cleanup to release that adopted lease. Use a sealed ticket/offer generation and compare state under the owner.

## Retirement states

```text
Reserved -> SentUnresolved -> GrantedHeld -> Adopted
                           -> ActualNoGrant
GrantedHeld -> RetirementNotSent -> RetirementSent -> ConfirmedRetired
```

Cancellation, reply loss and the 12-second deadline only change reporting/retirement intent. **No TTL removes an unresolved grant.**

Cleanup sends ReleaseControl or Unobserve with the actual held lease. Full keeps `RetirementNotSent` for the existing owner tick. A post-send error remains uncertain. Close custody only after exact adoption, actual matching retirement/revocation proof, or authoritative destruction of that Runtime generation. Source-finished and Phase are insufficient.

Adoption can cancel cleanup proven not sent. Reject adoption after retirement was sent until its outcome is known. Never release a later adopted/current lease using an older ticket. A checked ControlRequired/replacement result can prove that this exact old lease is no longer current, but cannot prove that the original Claim never granted it.

Shutdown retains unresolved entries and original source owners until those rules resolve them. Grant retirement never kills the process, ACKs output or replays input.

## Observe and recovery audit

Native Observe, observer-v2 attach and manager-v2 observer attach use the same grant-request path and exact Unobserve retirement. Generic `Service::call` cannot remain a Claim/Observe admission bypass.

RecoverStamped and ObserverRecoverStamped are not ownership-grant receipts. The actual producer contract permits an EXPLICIT fresh successful recovery on the SAME known current lease to supersede an unknown prior pending token without ACK or ACK-cursor advancement. Do not issue another Claim/Observe to infer that lease, automatically replay a recovery, fabricate a stamp, or treat a missing response as rejection. Initial attach holds its central grant offer through recovery; pre-adoption failure drops that offer into exact retirement. After adoption the real view book owns the actual lease/stamp and normal lifetime cleanup. See `docs/service-recovery-custody-audit.md` for the separate bounded audit; no blanket second lease/payload ledger is proposed.

## Required regressions

- Before-send Full versus actual no-grant rejection versus post-send timeout.
- Cancellation after queue admission; reply send failure; receiver dropped after send but before reading.
- Late actual Claimed/Observed captured and resumed without another grant command.
- Same-source/controller conflict blocked while unresolved; takeover by another actual owner cannot be released by stale cleanup.
- Cleanup Full, lost release reply, exact revocation and authoritative source destruction.
- Adoption before unsent cleanup; adoption rejected after release was sent.
- Actual lifetime/view-book adoption on native/legacy paths and current-binding mailbox adoption on M2, without claiming client consumption.
- Recovery result/barrier retention remains separately bounded and never synthesizes ACKs.
- Shutdown custody, exact source/controller correlation and no process kill.
