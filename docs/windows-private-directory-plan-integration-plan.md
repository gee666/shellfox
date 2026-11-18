# Private Windows DirectoryPlan integration — production plan for review

Status: PLAN ONLY after Reagan's second prototype review. No production entry/verifier/Root/UI/default policy is switched by this document. The current cfg(test,windows) prototype closes its tested ledger/handoff/reopen/assertion gaps but is NOT promoted. This plan replaces its one-original-missing-leaf assumption and caller-supplied proof guard.

## 1. Three separate contracts

1. **ApprovedRoot is scope.** It retains the actual directory, existing Access/revocation and the original shared ledger. It is not a continuing privacy promise.
2. **Private directory handoff is current/birth evidence.** A successful exclusive native create proves the reviewed privacy-at-create operation; an existing open proves only its current verified state. Neither turns a generic FileService into a permanently-private object.
3. **Sensitive use needs an explicit per-use guard.** Scoped metadata/journal consumers retain actual pins/evidence and check current private object state AND binding before/after reads, before staging and at final commit. No silent guarantee from the returned derived FileService, no ACL repair and no generic user-file workaround.

LocalDesktop/ApprovedFullAccount RW plans already have broad account authority; this is narrower use of their existing authority, not ApprovedRoot-only confinement. Selected roots retain their actual scope checks. No new cleanup authority comes from any of these contracts.

## 2. First candidate: fixed domain and actual compiled admission

The existing `directory_preparation_owner()` remains INFALLIBLE retention independent of admission. Keeper clones retain the SAME original issuer/ledger; fault/poison does not make the constructor fallible.

Actual phases are Idle/Active/Faulted/Preserved. Idle means no operation active, NOT a fresh generation/domain. One issuer is used per frontend preparation; independent New Window preparations do not share an issuer. No multi-root transaction/all-or-nothing isolation is claimed.

The CENTRAL compiled API is `FileService::acquire_directory_admission(&Arc<Self>) -> Result<DirectoryAdmissionGuard, DirectoryPreparationFailure>`. It claims Active under the ledger, RELEASES ledger, then internally acquires the REAL MUTATIONS guard and keeps the same keeper. Consumers borrow mutation() and MUST NOT reacquire. check_issuer is exact Arc+same ledger correlation only, not authority.

finish<T>(Result<T,DPF>) uses actual unresolved rows/created watermarks/explicit possible-effect classification. Success requires no unresolved attempt/poison. A structured pre-effect error returns Idle only with no possible marker or new effect/custody; post-effect/uncertain errors and unfinished/unwind Drop fault. No absence-of-FD/error-string inference or reset from fault.

PreserveActive is Busy without closing pins or waiting MUT. Idle/Faulted preservation seals Preserved under ledger FIRST, releases ledger BEFORE waiting MUT, then closes actual rows under MUT→ledger ordering. No global FileService revoke; independent derived pins/unrelated user I/O remain valid.

Exact source paths, signatures, origin queries and executed scope are in `directory-operation-guard-contract.md`. Reusable whole-session/generation APIs are an UNSELECTED alternative, not this candidate. No public keeper/API/Root/UI/default route flip is authorized. Original plan handles/snapshot are retained; no ambient replan to mint Windows proof.

## 3. Actual production entry owns guard and external keeper

The new Windows production entry remains private. It takes an actual owner-bound DirectoryPlan, not a File/path/bool/proof token. No public WindowsAtomicCreationAnchor constructor and no public mutation-guard argument.

The implementation:

1. Retains/clones the ORIGINAL preparation keeper OUTSIDE its internal catch_unwind boundary.
2. Claims the exclusive fixed-domain operation before waiting for MUTATIONS or performing native mutation.
3. Acquires the REAL FileService MUTATIONS guard internally and holds it over validation, create/open/reopen, postvalidation and handoff. Private helpers borrow that real guard and never reacquire it.
4. On ordinary error, returns typed DirectoryPreparationFailure retaining the same preparation/issuer/ledger.
5. On unwind, retains the pre-registered row/actual returned FD and returns or retains an interrupted typed preparation outcome. It never reports NoCreate/NoWrite just because no child FD reached the final return value.

The current prototype's caller-provided guard is useful test evidence, not the production entry contract. The public facade must not expose private proof construction.

## 4. Current prefix and multiple missing steps

Do not inspect only plan.original.missing.len()==1. At each boundary use the plan's CURRENT proven state:

- Revalidate the original capability, exact original ancestry/full identities and original component sequence.
- For a prefix created since the initial plan, require a live matching row in the ORIGINAL ledger for THIS fixed domain: parent full ID + exact leaf + actual child full ID + retained actual pin and immutable actual creator origin. An external existing directory/alias does not substitute for that row.
- Build the private Windows chain from the original retained handles plus actual live created-prefix handles, retaining no-delete-sharing pins. Fresh namespace opens may verify binding against those IDs; they never replace/regrant the original chain.
- Reserve bounded memory/retention for the remaining suffix BEFORE native calls. Existing component/ancestry/ledger bounds remain; Unsupported never becomes Disjoint or an adopted object.
- For each missing component, derive its actual parent from that proven prefix, validate ONE leaf, recheck active operation/domain, current RW/revocation/restrictions/cancellation, then perform exclusive native create.
- Revalidate after each committed step. The next step can use the newly created parent ONLY through its actual original ledger handle/full ID, not by replanning and accepting an unrelated prefix.
- Revalidate the full topology/relations at every consumer root boundary as before. Multiple sibling roots in one preparation share the original creation ledger; a different concurrent preparation cannot borrow it.

A partially completed suffix remains recorded on any later failure. No rollback-by-name or implicit adoption on retry.

## 5. Windows native result capture and handoff

Use the existing reviewed lower native directory creator under the DISTINCT actual DirectoryPlan/no-delete-chain proof. The generic File-based creator/parent verifier remains strict and unchanged.

Before entering the syscall, the original owner contains a bounded row with original parent handle/full ID, leaf and possible-artifact state. Preallocate the same shared CreationCustody/native result storage before mutation.

The syscall writes its returned FD into retained capture storage outside the local unwind boundary. Move the SAME actual FD/full ID into the original ledger before privacy/binding postvalidation, callbacks, cloning or derived-cap construction. If full-ID capture or any post-create validation fails, store the actual optional FD/evidence in the same row via real typed failure. No local-box-only keeper, fresh ledger, guessed ID, English error classification or test authority bool.

Exclusive collision does not adopt, repair or open the collided object as a successful creation. A returned directory handle keeps actual noDELETE/share3 behavior. Native unknown/error outcomes retain possible-artifact evidence rather than pretending no mutation occurred.

Only after current object privacy, exact binding and plan/issuer checks succeed may handoff clone the actual handle into a narrower normal derived scope. Preserve existing Access/revocation and SAME ledger. The result is NOT a continuing privacy promise; sensitive use must retain its explicit current-state guard.

## 6. Existing private target and explicit RW reopen

For zero missing components or an already-created current target:

1. Validate the actual owner-bound plan/current prefix and retained ancestor pins.
2. Use the distinct private Windows plan proof to open the exact leaf relative to the actual original parent, no-follow, with real no-delete-sharing pin. Do not route through a path regrant or weaken the generic parent verifier.
3. Verify CURRENT private object state and full identity against the original plan/live created-prefix row. Existing verification does not issue birth/created ownership or create a new ledger row.
4. If initial provenance is read-only, explicitly authorize the RW relative reopen under the original RW plan. Compare its full ID to the still-held original and retain the new RW pin; never relabel RO or infer RW from a label/number.
5. Recheck current privacy, binding and scope before handoff. Mismatch/unsupported state/collision fails closed without repair.

The private production receipt/guard remains private to the DirectoryPlan implementation; it cannot be minted from arbitrary File/path/bool inputs. Exact receipt-to-sensitive-consumer API must be agreed before activation, rather than silently adding a privacy promise to FileService.

## 7. Cancellation and unwind

The entry checks existing revocation/native restrictions and the actual caller cancellation context before native admission and between steps; cancellation can only deny. No cleanup-authority bool is accepted.

- Before first native call: typed rejection; do not claim absence if an earlier step already created something.
- After a native return: register actual custody FIRST, then observe cancellation or run postvalidation.
- During capture/unwind: original pre-registered row and any returned FD survive in the external keeper. A row without a child FD still means possible artifact.
- No retry under fresh authority or TTL erasure. A Faulted domain cannot resume directory preparation. Only the next sequential operation after a successful Idle finish may use live original-domain prefixes; it cannot create again to discover old outcomes.

Poison recovery is ONLY for conservative retained inspection/preservation. Do not clear poison to restore normal mutation authority.

## 8. Issuer-wide terminal preservation

Preservation is for the entire ONE preparation/issuer, not a per-plan success flag:

1. Reject Active without closing anything; do not force preservation through an in-flight operation. Once Idle/Faulted, recover poisoned locks only for conservative retained inspection/disposition.
2. Set PreservedTerminal BEFORE closing any owned child pins. Every later directory admission/open-or-create/preparation handoff on the issuer fails closed; no reset or fresh generation on that issuer.
3. Explicitly close only this owner's child/view pins while retaining original parent/full-ID/leaf evidence and truthful possible-artifact/stale state. Never unlink or claim cleanup/NoOwner merely because pins were released.
4. Independent already-derived pins remain valid and may keep names pinned. Do not revoke unrelated FileService user I/O or change its existing access/revocation merely to end preparation.
5. Closed/preserved rows cannot prove a newly adopted created prefix. Quota/evidence is not silently freed or rewritten as empty state.

The fixed-domain admission seal is now implemented privately and mechanically tested; this does not promote the private Windows candidate or switch any Root/UI/default route. Its native/per-use/shared-lifecycle integration remains subject to the proof and independent review below.

## 9. Required proof before promotion

Test the actual private production entry, not only a copied one-leaf test type:

- Zero/multiple missing components; legitimate current-created prefix progression; multiple sibling plans; external prefix/alias collision denial.
- Original ledger and SAME returned FD before every post-create fault/privacy/cancel/unwind hook.
- Existing root current verification, explicit scoped RW reopen and independent-pin lifetime.
- Current privacy/binding loss denies read/stage/commit through sensitive guards without repair; generic ApprovedRoot alone makes no such promise.
- Two shared-issuer preparations/operations cannot interleave; same-owner keepers work; issuer-wide preservation seals ALL later directory admission, including existing-target paths.
- Preserve releases only owned pins; independent derived pins stay valid; unrelated normal user I/O is not globally revoked.
- Generic creator still refuses the exact untrusted-mutating parent mask; assertions inspect the actual attempted leaf, including generic-denied.
- No default Root/UI policy switch and default ingress remains off.

Evidence is limited to owned Windows fixtures/current account and the reviewed kernel behavior. No admin/SYSTEM, hostile same-account process/handle, other-account execution, Unix/global VFS, ReFS, Go/GUI/network or privileged mutation acceptance is implied. After implementation/proof, obtain independent review AGAIN before any promotion or consumer/default activation.
