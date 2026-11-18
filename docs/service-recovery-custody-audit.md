# Service recovery custody audit

Status: source audit against the actual Runtime contract, not Service/native controller acceptance or ingress activation. No new recovery/lease ledger is implemented by this document.

## Producer facts

Read in full: `crates/shellfox-runtime/docs/grant-recovery-failure-contract.md`.

- A missing response, panic, timeout or disconnect is not a native error result.
- Successful RecoverStamped/ObserverRecoverStamped keeps the SAME real lease, installs a new opaque pending token, resets native delivery credit/sent state, and does NOT advance the ACK cursor. Input/output remain behind their real pending gates.
- Ordinary pre-replacement errors preserve the previous pending token. The current synchronous owner invariant leaves no reachable ordinary fallible post-token-commit stage.
- An EXPLICIT fresh successful stamped recovery can supersede the pending token on that same known current lease. It needs no fake ACK of lost bytes. The old token becomes invalid; only actual consumption and exact ACK of the new complete recovery releases its barrier.
- Native legacy ObserverRecover is not an allowed manager stamped-recovery downgrade.

## Current production caller inventory

The Service production stamped-recovery calls are initial attachment in `manager_attach.rs` (controller or observer) and `remote_v2_attach.rs` (observer). Both now:

1. Obtain an actual ControllerGrantOffer/ObserverGrantOffer from the ONE central ownership-request ledger.
2. Retain the offer while installing a provisional real mailbox and requesting stamped recovery on the offer's exact actual lease.
3. Validate/install the actual recovery result, revalidate current binding/policy and perform the Service's atomic adoption/book commit.
4. On ANY pre-adoption failure, discard only provisional bookkeeping. The offer drop retains central exact-lease retirement, including a late recovery success whose bytes were not returned.

There is no fabricated epoch, inferred replacement lease or mailbox-local Claim ledger. The provisional mailbox cannot locally Release/Unobserve while the central offer still owns retirement.

## Lost initial recovery result

If generic Service wait loses/times out the initial recovery result, it cannot call that a failed/no-op native recovery. The initial attach fails before adoption, purges provisional refs/data and drops its actual ownership offer. The central ledger retains that exact lease until matching ReleaseControl/Unobserve, revocation or authoritative source destruction. Runtime can retain a real pending snapshot token meanwhile; no bytes are ACKed. A late recovery payload may be discarded without pretending it was consumed, because that unadopted lease is being retired by its existing custody owner.

Therefore the current initial-attach paths do NOT require a second growing recovery payload or lease ledger. Their ordinary bounded response/delivery limits and central retirement remain in force. Generic recovery response loss must never substitute a guessed lease or a successful-looking empty recovery.

## Reply lost after adoption

After Service atomically adopts, the real current-binding mailbox/lifetime book owns the actual lease and exact current stamp. Failure to deliver/read the RPC response does not establish byte consumption. The existing transport trailer is not a terminal-data ACK. The view keeps its real pending barrier and is retired through its existing lifetime/binding disposal, not by an old central-offer drop.

Current wire recovery limits are explicit (`observer_v2::MAX_RECOVERY_BYTES`, shared by manager-v2, currently 4 MiB). Mailbox/view counts and owner request channels are bounded. These are not grounds to add an unbounded historical snapshot queue.

## Future explicit same-lease replacement: review requirements

Current source does not provide a separate adopted-view Service stamped-recovery path; initial attachment is the inventory above. If one is added, prefer one owner-serialized current recovery operation/state per exact real role/lease, with bounded admission, rather than a blanket payload ledger:

- Validate real current binding/policy, the already-adopted actual lease and requested profile. Never reacquire it via Claim/Observe because recovery was uncertain.
- Capture the exact request ID; distinguish actual ordinary failure from missing outcome.
- Fence superseded queued delivery/opaque refs. Do not keep serving an old token as current after an unknown replacement success.
- Ordinary error may retain the prior pending token; an explicit fresh success replaces it. Missing outcome stays fenced until an explicit fresh same-lease success or exact retirement.
- Install only the actual current operation's response. Never let an older late response overwrite a newer token/ref.
- Bound any temporarily retained bytes to the negotiated recovery limit and current-operation budget. Keep offered-data ACK correlation in the existing real delivery book.
- No automatic mutation retry, scalar-ACK shortcut, fake zero-sequence consumption, legacy downgrade or new control grant.

This is a separate API/behavior review, not permission to enable ingress or silently change V1 shapes. Native Service acceptance and full recovery-consumption regressions remain separate validation gates.
