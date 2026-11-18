# Approval crash barrier: proposal for review

**Not approved or implemented.** Native approval writes remain gated. This document does not change the records V2 schema, fixed `control-full-account-v1` policy, current readers or legacy recovery rules.

## Existing authority and compatibility

The exact namespace remains `remote-v1/approvals.json` and `remote-v1/write-pending`, under the original retained native control-child capability. No flattening, label-to-path conversion, permission repair or arbitrary unlink is permitted.

Today, any existing `write-pending` makes startup fail closed. Empty, malformed, inaccessible, oversized, linked, unfamiliar or otherwise ambiguous marker objects are not removable recovery hints. A read hash, matching pair of files, timestamp, transaction UUID, current Go lock, or current native owner is not proof that a previous transaction committed.

Read-only capability migration can retain those existing rules. A V1 record migration that requires saving before installation must continue to fail rather than install normalized grants without an accepted durable save.

## Proposed versioned marker, not new approval records

A future known marker format could retain:

- Marker format version and a fresh transaction ID.
- Parent committed transaction ID and the parent's exact approvals byte digest.
- Intended approvals byte digest and byte length, record schema V2 and the fixed policy literal.
- Explicit `pending` or `committed` phase.
- The exact lineage checkpoint whose durable authority was established by an independently accepted bootstrap/commit rule.

The digest binds the exact encoded approval-document bytes, not a reconstructed or reordered JSON value. The format must be closed and bounded. Native handle identity can support same-run binding checks, but is not a portable transaction identifier or reboot commit proof. A self-declared ownership epoch is diagnostic only.

A marker in this format would occupy the existing `write-pending` leaf. Older readers therefore continue to deny rather than ignore it. An unversioned legacy marker never becomes a valid transaction merely by adding fields or successfully reading its corresponding approvals document.

**This shape is insufficient by itself.** A committed marker plus a matching hash does not establish historical commit authority. A missing marker cannot distinguish an untouched legacy installation from an interrupted migration or rollback. No new reader may silently fall back to legacy approval installation in that ambiguity.

## Required authority decision before implementation

One of these must be explicitly accepted before new-protocol startup can install grants:

1. A platform/filesystem durability contract with ordered, confirmed file and namespace durability, plus an accepted durable migration/checkpoint rule that distinguishes never-migrated legacy state from missing/rolled-back transaction state; or
2. A separately accepted external monotonic/durable commit authority; or
3. Fail-closed local reapproval/recovery, without automatic installation from ambiguous on-disk state.

Neither a callback nor a marker field supplies this authority. Current Windows publication reports `PrivateDurability::Unconfirmed`; it cannot be relabelled confirmed or treated as a power-loss commit guarantee. The current primitives alone do not resolve this decision, so the proposal is not ready for activation.

## Candidate transaction order, conditional on that decision

The Service retains its original ownership token, root lifetime, creation custody and bounded transaction owner before the first fallible operation. Only one transaction may proceed per store. Any uncertain operation quarantines further writes; there is no mutation replay.

1. Read the exact legacy/current marker and approvals through bounded retained-capability operations, checking current Service ownership before and after reads. Validate the accepted baseline lineage without granting from a hash alone.
2. Prepare and publish a private-at-birth `pending` marker with the base and intended document digests. Require the durability guarantees chosen above before any approvals replacement.
3. Prepare and publish the exact approvals V2 bytes, using the accepted expected document version. Preserve actual `NotPublished`, `Published(Confirmed/Unconfirmed)` and `Uncertain` outcomes. Never infer rollback from an error.
4. Only after required document durability is confirmed, publish the matching `committed` marker with the same transaction ID and parent lineage. Require the chosen namespace/checkpoint durability before acknowledging a durable transaction or installing a newly approved grant.
5. Preserve all failed/uncertain owned attempts with the original bounded transaction owner. Stage cleanup uses only original creation custody. Never delete the legacy marker or another actor's stage to unblock startup.

Revocation denies the in-memory old grant before I/O, as it does today. A failed transaction never restores that epoch. A newly approved grant is not installed before the accepted transaction completion point. A process crash before completion may leave an approvals document that contains the desired state, but recovery must still deny when historical completion is not established.

## Crash and interference matrix

In this table, **deny** means no approval installation and no automatic mutation retry. It does not claim that no write occurred.

| Observation / interruption | Required result |
| --- | --- |
| No marker, no approvals, no accepted migration history | No existing grants. First-write migration/checkpoint authority still required; absence does not certify a completed transaction. |
| No marker, valid legacy approvals | Read-only legacy handling only under the unchanged accepted legacy rule. Never use this fallback for state known or possibly migrated to the new protocol without a trustworthy discriminator. |
| Marker missing after an accepted new-protocol checkpoint | Deny, including if approvals hash matches an older document. No historical commit inference. |
| Empty/unversioned legacy `write-pending` | Deny. Preserve it; do not delete, reinterpret or overwrite it as transaction initialization. |
| Torn/invalid/oversized/unknown-version marker or unknown fields | Deny before changing approvals or granting. Preserve evidence. |
| Valid marker but missing/torn/invalid approvals | Deny. Do not manufacture an empty approval set as recovery. |
| `pending`, approvals still equal base | Deny. No inference that the attempted approval write was never submitted. |
| `pending`, approvals equal intended digest | Deny. Matching bytes do not prove commit or durability. |
| `committed`, approvals digest/size/schema/policy mismatch | Deny. No rollback to parent approvals and no marker repair. |
| `committed`, matching approvals, lineage absent/unknown/forked | Deny. Hash agreement is not historical authority. |
| `committed`, matching approvals and lineage, but required durability/checkpoint evidence unavailable | Deny; do not relabel an unconfirmed publication as committed. |
| First marker creation fails before native create | Return actual rejected/pre-create result. No grant; do not claim a transaction committed. |
| First marker creation may have occurred or validation fails afterward | Retain the typed preparation failure and original keeper. Deny/refrain from approvals publication; preserve uncertain artifacts. |
| Stage write partially fails or its response is lost | Quarantine that transaction; no write replay or fabricated zero-write result. Keep original attempt/custody. |
| Pending-marker rename is `Uncertain` | Do not write approvals. Quarantine; preserve both current observations and original attempt. |
| Pending-marker publication succeeds, directory durability unconfirmed | Do not proceed to approvals replacement when the accepted protocol requires durable ordering. No power-loss completion claim. |
| Approvals rename is `NotPublished` | Keep durable pending marker; deny grants. Cleanup only the exact owned stage when its primitive permits. |
| Approvals rename is `Uncertain` | Deny. Do not publish a committed marker based on a fresh hash read. No retry of the approvals replacement. |
| Approvals published, file sync known, directory durability unknown | Distinguish published bytes from durable namespace state. Do not acknowledge the stronger guarantee. |
| Committed-marker publish fails/uncertain | Deny new-protocol startup/installation. The matching approvals document is not sufficient evidence. |
| Ordinary process crash after file sync, before rename | Treat retained pending/unknown state as ambiguous; file sync does not publish a namespace entry. |
| Process crash after rename, before directory/checkpoint durability | A subsequent matching read establishes current bytes only. It does not prove the earlier required durable completion. |
| Power loss at any boundary | Apply only the explicitly accepted storage durability contract. Where guarantees are unconfirmed, deny rather than assume process-crash semantics. |
| Ownership lost before a read, or during it | Refuse/withhold the read result, even if the cap's mutation fence would permit read access. |
| Ownership lost before pending/approvals/committed publication | Native mutation fence rejects the next operation; preserve the actual attempted operation's outcome and custody. No rollback assumption. |
| Ownership lost just after a confirmed commit | Retain the historical result without granting continuing authority; new owner recovery still needs its accepted lineage/durability evidence. |
| Concurrent file replacement/content change | Exact expected-version/binding checks reject; do not replace the new object or retry with its freshly sampled version. |
| Downgrade to old reader with any new-protocol marker | Old existence check must continue to deny. No marker deletion to make downgrade appear safe. |
| Upgrade from old reader with an existing pending marker | Deny, including if the old approval document parses and hashes correctly. |
| Upgrade from valid legacy records requiring V2 migration | Save-before-install requirement remains. Without an approved write protocol, do not install a silently normalized grant. |

## Review and test gates

Before changing schema/readers/writes, review the migration discriminator and historical authority decision, the complete marker shape, first-creation behavior, actual platform durability semantics, and the treatment of retained ambiguous state. Unit/hash tests cannot approve those assumptions.

Then add deterministic boundary tests for every matrix row, exact ownership-loss admission, corrupt/legacy fixtures, no mutation on rejected preparation, no replay after unknown publication, typed-keeper retention across failures, and restart recovery. Process-crash injection and power-loss guarantees must be named separately. Darwin compile/Clippy is not Darwin runtime evidence. No Service-lib relink or new native/Go runner is authorized by this proposal.
