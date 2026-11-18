# Reusable directory sessions — UNSELECTED future alternative

Status: NOT THE FIRST CANDIDATE. Do not implement or consume this alternative's signatures. Root selected Walter's actual fixed-domain compiled producer in `directory-operation-guard-contract.md`: infallible keeper, acquire_directory_admission, owned real guard and generic finish<T>(Result<T,DPF>). No reusable session/generation or public-route change is approved. Retain this owner-authored document only as a separately reviewable future alternative, not another source ABI.

## 1. Infallible retention keeper is unchanged

```rust
impl FileService {
    pub fn directory_preparation_owner(self: &Arc<Self>) -> DirectoryPreparationOwner;
}
```

This existing infallible method only retains the SAME original issuer/ledger. It remains usable while Idle, Active, Faulted, Preserved, revoked or after poison. Keeper clones are retention views, NOT admission/session acquisition. Existing retention(), verify_retained() and explicit preserve_stale() do not acquire a new session. No fallible/infallible envelope constructor or fabricated failure is introduced.

## 2. Distinct private fallible session API

Proposed exact crate-private names:

```rust
pub(crate) struct DirectoryPreparationSession; // not Clone; fields private
pub(crate) struct SessionDirectoryPlan<'plan>; // borrowed original + private generation
pub(crate) struct DirectoryOperation<'op>;      // actual guard + keeper, private constructor

impl DirectoryPreparationOwner {
    pub(crate) fn acquire_session(&self)
        -> Result<DirectoryPreparationSession, DirectoryPreparationFailure>;
}
impl DirectoryPreparationSession {
    pub(crate) fn bind_plan<'plan>(
        &self, original: &'plan DirectoryPlan,
    ) -> Result<SessionDirectoryPlan<'plan>, DirectoryPreparationFailure>;

    pub(crate) fn run<T>(
        &mut self,
        bound: &SessionDirectoryPlan<'_>,
        work: impl FnOnce(&mut DirectoryOperation<'_>)
            -> Result<T, DirectoryPreparationFailure>,
    ) -> Result<T, DirectoryPreparationFailure>;

    pub(crate) fn finish(self) -> Result<(), DirectoryPreparationFailure>;
}
impl DirectoryOperation<'_> {
    pub(crate) fn plan(&self) -> &DirectoryPlan;
    pub(crate) fn current(&self) -> Result<DirectoryWalk>;
    pub(crate) fn mutation(&self) -> &std::sync::MutexGuard<'static, ()>;
}
```

`Result<T, DirectoryPreparationFailure>` above is std::result::Result, NOT the one-parameter FileError alias. DirectoryWalk is the existing internal retained walk, not a new public authority type. No session/generation/operation constructor is public, serde or accepted from a boolean/path/File. No new wire/dependency/runtime permission.

The session stores a clone of the EXISTING keeper and a private checked monotonic generation in the SAME CreationLedger. It is not a second owner/ledger. Acquisition requires original RW/scope/revocation/restriction checks and Idle admission. A second acquisition while Active/Faulted/Preserved is rejected before mutation; it does not join an existing session because it shares an Arc.

## 3. Binding preserves original scope and snapshot

bind_plan verifies exact original issuer Arc identity, same ledger, active session generation and original logical RW scope. A derived capability with the same ledger is not automatically the same issuer/scope.

It BORROWS the supplied actual DirectoryPlan. It does NOT call plan_directory again, reopen a configured label or replace original handles/identities/names/missing suffix. It performs no ambient replan. Ordinary readonly/unbound planning remains available but cannot construct a SessionDirectoryPlan or new Windows proof.

The bound wrapper holds only the borrowed original plus private generation/issuer binding. Old bindings cannot operate in a different session. Five original plans may bind to one exclusive session and run sequentially; another preparation cannot interleave between those five operations.

## 4. SAME ledger phase, not FileService revocation

```text
Idle
Active { generation, operation_in_flight, session_start_watermark }
Faulted { generation, reason }
Preserved                         // irreversible on this issuer/ledger
```

Internal fault reasons are bounded diagnostics (Interrupted, PostEffectFailure, UncertainNativeOutcome, PoisonedState), not proof of no creation. All session/operation checks use the original ledger, not path text or an authority callback.

run requires the matching live bound generation and no other operation in flight. It claims the operation BEFORE waiting for global MUTATIONS, then acquires the REAL global guard internally. Failure to acquire a poisoned normal lock is not authority recovery. A concurrent run/acquisition is rejected, not queued into another preparation.

The operation owns a clone of the original keeper and its real guard OUTSIDE its internal catch_unwind boundary. The candidate receives only this existing private operation context; it never constructs a guard/proof token. mutation() is only a crate-private borrow of that actual guard, not a public authority API.

## 5. Begin/completion/error/unwind semantics

- Session acquisition: Idle → Active with new generation. No filesystem effect.
- Binding: readonly, no admission mutation beyond checking the active generation. A rejected binding leaves the explicit session active until finish/drop; it cannot open another preparation.
- run success: clear operation_in_flight; remain Active so the other bound plans use the SAME session. Do NOT return Idle after each successful child/root.
- Actual pre-effect run rejection with no created/uncertain/retained row anywhere in this session: may close the session back to Idle, invalidating its bindings. This uses real ledger watermarks/rows, not error strings or absence of a returned FD.
- Typed failure after ANY creation in this session, retained native custody, an entered unresolved attempt, or uncertain result: → Faulted. Earlier successful roots followed by a later pre-syscall rejection still count as a post-effect preparation failure.
- Panic/unwind: capture/retain the pre-registered row and any actual returned FD first; → Faulted. The external keeper survives. Return a typed interrupted preparation failure; never NoCreate/NoWrite/NoOwner.
- Dropping a live/uncompleted session or operation: quarantine Faulted conservatively, not Idle. Drop performs BOOKKEEPING only, no unlink/permission repair and no automatic cleanup retry. A normally finished/closed stale generation cannot fault a later generation.
- finish succeeds only for its Active generation with no operation in flight, no unresolved attempts and completed successful handoffs; then → Idle and invalidate generation. Created pins/evidence remain owned; finishing is not cleanup.
- finish on fault/uncertainty preserves the original keeper in the typed failure, not a new admission. No reset from Faulted through a convenience constructor.

No new session while Faulted until a separately proven disposition. Initial supported disposition is explicit terminal preservation; a future reset-after-confirmed-owned-cleanup needs its own exact resource proof, not a boolean.

## 6. Creator origin is a fact, never continuing privacy

```rust
pub(in crate::directory_plan) enum DirectoryCreatorOrigin {
    Ordinary,
    PrivateBirth,
}
```

Add the private origin to each actual created row, plus the session generation that created it (legacy/unbound rows have no candidate-session generation). Current-state existing verification does not create a birth row.

Proposed exact low-level changes for the coordinated consumer batch:

```rust
CreationLedger::commit_created(
    &mut self, attempt: usize, child: HandleIdentity, directory: Dir,
    origin: DirectoryCreatorOrigin,
) -> usize;
CreationLedger::created_origin(
    &self, parent: &HandleIdentity, leaf: &OsString, child: &HandleIdentity,
) -> Option<DirectoryCreatorOrigin>;
```

The ordinary mkdir path records Ordinary. Actual successful shared private creator/native atomic capture records PrivateBirth only from its REAL returned FD/identity and reviewed creation operation. No relabel after chmod/ACL repair or because today's directory is private. The enum is internal recorded fact, not public authority or a test-minted permission flag.

`DirectoryOperation::current()` verifies the original snapshot and requires any since-created prefix row to be live, match full parent/leaf/child IDs and belong to this SAME generation. It returns the existing DirectoryWalk for actual retained chain use, not a replacement original plan. The candidate may use an Ordinary prefix as proven namespace scope/pin only where its contract permits; it must NEVER describe that prefix as privately born. Every sensitive handoff/use still checks current private state/binding.

All native attempts reserve capacity/register in the original ledger before syscall. Typed error capture and actual-FD commit stay before clone/postvalidation/faults. New session/origin metadata must not discard a preallocated failure/custody payload.

## 7. Existing target and open_or_create paths

Admission is checked at the high-level entry BEFORE all existing/open/create/reopen/handoff branches, including zero missing components. Existing-private verification is not an exemption from a Faulted/Preserved issuer.

The private retained child helper runs only under its caller's already-active operation/real MUTATIONS guard; it must not create a nested session or reacquire the lock. Existing readonly open → authorized RW relative reopen → current privacy/full-ID/binding validation stays under that SAME operation. Generic File-based parent verification remains strict.

## 8. Keeper preservation is independent and issuer-wide

Keeper APIs never call acquire_session and remain accessible after fault/poison.

- Active: preserve_stale returns refusal WITHOUT closing ANY pins or changing admission. This includes an Active session between bound-plan operations.
- Idle or Faulted: set Preserved BEFORE closing the issuer preparation's own child pins; retain original parent/full-ID/leaf/evidence and quota.
- Preserved: idempotent conservative preservation/inspection only. Every later directory/session/open_or_create preparation admission fails closed, even existing-target paths.
- Do not set FileService.revoked or revoke unrelated user I/O. Independent already-derived pins remain valid and can still prevent namespace removal; releasing this owner is not cleanup authority.
- Poison is recovered only for retention inspection/quarantine/preservation, never cleared to resume normal mutations.

## 9. Staged consumer batch and validation

Before a PUBLIC API/routing change:

1. Root reviews this exact contract.
2. Walter implements private phases/session/origin APIs and publishes actual compiled names; old infallible keeper unchanged.
3. E consumes those actual exports in the private Windows candidate worker, multi-current-prefix/missing/existing paths. No guessed fields, test authority helper or production verifier switch.
4. Coordinate private origin-callsite updates in real normal/private creators, prototype/candidate and owned tests. Coordinate existing/privatechild admission without nested session/lock.
5. Validate active preservation refusal, typed post-effect faulting, unwind/drop quarantine, same-generation five-plan sequencing, second-session denial, original snapshot binding, terminal existing/create denial, origin truthfulness and independent derived-pin/user-I/O behavior.
6. Only after candidate/private/per-use privacy/shared-lifecycle/default-composition tests and independent review may Root decide any public facade/consumer/default wiring batch. A future frontend multi-plan facade must use ONE session across its actual original plans; no per-root hidden reacquisition to evade exclusivity.

No UI/default/ingress switch, public keeper signature change, Host/VFS approval, admin/SYSTEM/hostile-same-account process closure, ReFS/Unix VFS/other-account/Go/network/GUI or Service-lib test relink is authorized here.
