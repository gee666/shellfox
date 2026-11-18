# Fixed-domain directory admission — actual compiled producer contract

Walter owns this coordination record following Root's central decision. The signatures below match the actual compiled source. They supersede the earlier finish-only/acquire_directory_operation and reusable-session proposals. E must consume these source exports, not another draft. Windows candidate/public route, UI/default policy activation remains unapproved.

## Actual crate-private API

```rust
impl FileService {
    pub(crate) fn acquire_directory_admission(self: &Arc<Self>)
        -> std::result::Result<DirectoryAdmissionGuard, DirectoryPreparationFailure>;
}
impl DirectoryAdmissionGuard {
    pub(crate) fn check_issuer(&self, issuer: &Arc<FileService>) -> files::Result<()>;
    pub(crate) fn mutation(&self) -> &std::sync::MutexGuard<'static, ()>;
    pub(crate) fn mark_effect_possible(&mut self);
    pub(crate) fn finish<T>(
        self, outcome: std::result::Result<T, DirectoryPreparationFailure>,
    ) -> std::result::Result<T, DirectoryPreparationFailure>;
}
```

Type paths inside Files: `directory_plan::create::{DirectoryAdmissionGuard, DirectoryPhase}`. Guard fields/constructor are private; it is not Clone. No public mutex/proof token or new runtime permission.

## Fixed domain and lock order

- Existing `directory_preparation_owner()` is unchanged and INFALLIBLE. It retains the SAME issuer/ledger and stays usable after fault/poison; keeper cloning is not admission.
- ONE fixed preparation domain per original issuer/ledger. `Idle` means no operation active, NOT a fresh generation or independent preparation. No reusable session API is selected.
- Acquire claims `Active` under the ledger, RELEASES that lock, then internally acquires the REAL global MUTATIONS guard. The returned guard owns that real guard and same retention keeper. E must borrow mutation(), NEVER reacquire MUTATIONS.
- Exclusive admission spans before the mutation wait through completion. A concurrent same-domain acquisition refuses instead of interleaving. Frontend uses one issuer per preparation and does not share it across independent New Window preparations.
- This is NOT multi-root transaction/all-or-nothing isolation. Sequential roots operate in the same fixed domain.
- check_issuer requires exact Arc AND same ledger identity only. It is NOT RW/scope/revocation/native-owner authority; native checks remain explicit.

## Effects, finish and drop

- mark_effect_possible is infallible/nonlocking, before a potentially-effecting native create. Read-only metadata queries/existing-object reopens alone are not creation claims.
- Success may return Idle only with no unresolved attempts and no poison, after the caller's actual validation/handoff. It does not clear earlier rows or create another preparation.
- An error returns Idle only with structured pre-effect proof: no explicit possible-effect marker, no newly created row and no entered/retained unresolved attempt. English errors or no returned FD are not proof.
- An explicit possible-effect marker is NEVER erased merely because no rows remain. Post-effect/uncertain errors, unresolved custody and poison fault/quarantine.
- An unfinished/error/unwind Drop faults its matching Active operation conservatively. Drop only updates metadata: no nested preserve, mutation-lock acquisition, unlink, revoke, cleanup retry or poison clearing.
- A normal successful finish disarms Drop before releasing the actual owned mutation guard. A stale finished guard cannot fault a later operation.

## Preservation

- Active preservation is Busy and closes nothing; it does not wait for MUTATIONS.
- Idle/Faulted preservation sets Preserved under the ledger FIRST, RELEASES the ledger, then acquires global MUTATIONS and closes actual rows under MUTATIONS→ledger ordering.
- Preserved is terminal on this issuer/ledger. Faulted and Preserved cannot be rearmed by acquisition/convenience construction. No global FileService revoke: unrelated user I/O and independent already-derived pins remain valid under their own checks.
- Keeper inspection/preservation recovers poison conservatively without clearing it or restoring normal mutation authority. Preserve is not cleanup authority/NoOwner proof.

## Actual origin API

```rust
pub(in crate::directory_plan) enum DirectoryCreatorOrigin {
    Ordinary,
    PrivateCreator,
}
CreationLedger::commit_created(index, child_id, directory) -> usize; // ALWAYS Ordinary
CreationLedger::commit_created_with_origin(index, child_id, directory, origin) -> usize;
CreationLedger::created_origin(parent_id, leaf, child_id) -> Option<DirectoryCreatorOrigin>;
CreationLedger::clone_created_matching(
    &self, parent: &HandleIdentity, leaf: &OsStr, child: &HandleIdentity,
) -> files::Result<Option<Dir>>;
```

The type is re-exported under `directory_plan::create`. Query requires a LIVE exact original-ledger row. Actual sealed shared private creator completion records PrivateCreator; ordinary mkdir/generic commit remains Ordinary. The candidate's reviewed native capture may record PrivateCreator only from its actual FILE_CREATE/protected-SD/returned-FD path. Current private permissions, intention, uncertainty or existing reopen never rewrite/mint origin. Origin is not continuing privacy. clone_created_matching requires a live exact row, queries full native identities of its actual retained parent and child, propagates query failure as Err, then clones the ORIGINAL held directory. No index guessing, replan or permission proof. A preserved/missing/nonmatching row returns None; an independent earlier clone remains its own real pin.

## Integration boundary and checks

Normal DirectoryPlan creator logic now uses the guard without weakening generic File-based verification. The new Windows candidate route remains UNWIRED. Existing/privatechild high-level operations must acquire once before either branch and use mutation() through reopen/create/postvalidate/derive; helper does not acquire nested admission or lock.

Actual checks: offline/locked Files lib/tests metadata and strict all-target Clippy pass. Focused admission mechanics3/3, retained-ledger panic keeper1/1 and stale-child-pin preservation1/1 pass. These are not Windows birth/kernel/ACL/Go ownership/ReFS/global VFS acceptance. Promotion requires candidate/per-use privacy/shared-lifecycle/default-composition tests and independent review.
