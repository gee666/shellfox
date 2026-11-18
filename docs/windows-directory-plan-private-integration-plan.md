# Windows private DirectoryPlan integration — review plan

**Not promotion approval. No production edits, default-policy switch or new tests were performed for this plan.** The reviewed private prototype remains `cfg(test, windows)`. Its one-originally-missing-leaf constructor is not a production implementation.

## 1. Guarantee and unchanged boundaries

The proposed Windows branch provides original RW/scope/revocation admission, exact native object/binding custody, atomic private birth for an actual successful exclusive create, and current-private verification at handoff. An existing-object reopen provides current verification only, never birth or created ownership.

The resulting general `ApprovedRoot` remains filesystem scope. It does **not** certify continuing privacy, historical privacy of existing objects, Service/Go ownership, remote permissions, successful publication or resource cleanup. Sensitive consumers need a separate retained per-use privacy/binding guard.

The specific alternative for review is a current-user-owned, fully pinned DirectoryPlan parent that has an otherwise-refused mutating allow ACE. Acceptance would depend on the whole plan-bound NTFS proof: original no-follow ancestor identities, exclusive kernel creation with protected non-inherited child SD/current owner, capture of the actual returned object, and actual no-delete-sharing pins. It is not a claim that the parent's ACL is private or non-mutating. An ordinary principal may still race to occupy a missing name or cause denial; collision/replacement is refused rather than adopted. Unknown ownership, missing pins or any unproved condition still reject. This is a separate Windows guarantee, not removal of the generic parent check.

- Keep generic `create_private_directory_at(&File, ...)` and generic existing-private opens strict. No broad-ACL bypass is added there.
- Do not expose `WindowsAtomicCreationAnchor`, a constructor accepting arbitrary `File`, a caller-supplied bool, or a purported `MutexGuard<()>` from another mutex.
- Keep the Host allowlist, privilege/elevation boundary and wire schemas unchanged. Frontend never creates its Host plan.
- Initial promotion scope would be the reviewed NTFS behavior only. Unknown filesystem rules and ReFS remain unsupported until separately reviewed. Do not infer support from full-ID availability alone.
- No hostile-same-account, administrator, privileged handle manipulation, global VFS confinement, or Unix namespace guarantee is added. Unix/macOS keep their existing strict private-creator policy.

## 2. Public entry and real internal transaction

Keep the existing signature:

```rust
DirectoryPlan::create_private_approved_root(&self)
    -> Result<Arc<FileService>, DirectoryPreparationFailure>
```

The real public entry owns the following internally, before the first native call:

1. The original issuer Arc and `DirectoryPreparationOwner`, held outside the internal unwind boundary.
2. A private issuer-preparation admission lease, acquired before waiting for `FileService::mutation_lock()`.
3. The **actual** `MUTATIONS` guard obtained by that function, not a caller-provided guard-proof argument.
4. Preallocated created/failure rows, cloned handles/names, capture custody, and a safe preallocated unwind diagnostic.

Only a private transaction object constructed by this entry may call the Windows plan-bound native helper. Its fields and constructor stay private to Files; the helper does not become a new authority issuer. Existing under-lock helpers remain internal and cannot be used as proof that an arbitrary caller holds the real application mutation lock.

The keeper and transaction state survive the internal `catch_unwind`. On a genuine unwind, record/retain the current attempt, seal this issuer's preparation admission, and return a real typed retained failure using the original owner. Do not manufacture a failure merely to obtain a keeper, log/discard the panic custody, clear poison, or let a panic payload become public diagnostics. Desktop's already-retained outer keeper remains an additional caller boundary, not a substitute for the production entry's ownership.

## 3. Issuer-wide admission and terminal preservation

Add a private preparation state shared with the original creation ledger and its derived capabilities:

- **Open**: a sequential next DirectoryPlan operation in this preparation may enter.
- **Active**: one entry owns the preparation lease. Concurrent entry through any plan/clone sharing that issuer is refused before native creation, not silently serialized as another preparation.
- **FailedRetained**: no new directory preparation is admitted; the original owner retains the failure.
- **Preserved**: terminal for this preparation. Every DirectoryPlan/private-directory admission on that issuer is sealed.

A successful entry explicitly releases its lease back to Open so the same five-plan frontend preparation can create its remaining roots sequentially. Failure/unwind does not restore Open. Preservation seals admission before draining/marking ledger custody. A previously registered in-flight syscall can still produce an artifact; its row remains authoritative until its outcome is retained. No later prefix step or handoff may pass after sealing wins the admission boundary.

Use a fixed lock order for admission, actual mutation, and ledger access. Preservation must not acquire the ordinary mutation lock; it recovers ledger poison only for retained verification/preservation, without clearing poison or restoring ordinary authority. Handoff and preservation need an explicit linearization point: either handoff completed first and the derived pin remains valid, or preservation won and the entry returns a retained failure.

This state seals **directory preparation**, not all account I/O. Do not call `FileService::revoke()` on the issuer or unrelated derived capabilities. Independent derived scope/pins remain valid; they are not continuing-private certificates. Existing ordinary user-file/folder operations are not silently reclassified as preparation or given a new permission. The pre-existing ordinary-operation poison fence also must not be cleared.

## 4. Current-created prefixes and multiple missing steps

Replace the prototype's reliance on `plan.original.missing.len() == 1` with the actual `plan.current()` result under the production transaction.

1. Revalidate all original no-follow identities. Every component absent originally but present now must be proved by the **same issuer's live created-prefix ledger**. A stale/closed ledger entry, outside insertion, or string equality is not adoption evidence.
2. Bound the remaining components using the existing path/component limits and the ledger's total bound. Reserve all successful/failed storage for the current suffix before the first native create.
3. For each missing component, use the current held parent and its full identity. That parent may be an original existing component or an earlier actually created/recorded prefix, including a prefix created by another sequential plan in this same preparation.
4. Retain the relevant current ancestor chain, identity vector, parent identity and single leaf before the call. Do not derive a fresh anchor from the original last-resolved parent or only clone the test prototype's original arrays.
5. Recheck original scope/revocation, admission, parent binding, permitted owner and native filesystem support at the call boundary. The separate plan-bound Windows guarantee permits no relaxation of generic File-based creator policy.
6. Register the attempt in the original ledger before entering the native call. Use exclusive `NtCreateFile(FILE_CREATE)` with the explicit protected creation SD and actual no-DELETE/share-3 directory pin. Validate the native created disposition, not just a successful handle.
7. Capture any returned FD into the already-owned custody slot before full-ID inspection or other fallible work. Move the **actual** File/full identity into the original ledger before clone, reopen, postvalidation or fault injection. A missing FD after syscall entry remains possible-artifact evidence.
8. Recompute/check current binding after each step; check admission again before another native call and at handoff. A collision is not permission to open/adopt the named object.
9. If no components remain, distinguish an originally existing target from a current target proven created by this issuer. Both require current-private verification and a real RW/no-delete reopen where needed. Neither branch silently repairs privacy or invents a birth receipt.

Desktop still checks its complete Host/UI/device/run/editor physical relation matrix before and after every root operation. The Files loop handles each native prefix boundary; Desktop must not grow a second OS walker.

## 5. Existing reopen and scope handoff

Production reopen must use the current retained parent/leaf, expected full identity and actual RW access/pin provenance. Verify current directory kind, no redirect, ownership, protected private policy and named binding before and after handoff. Recheck scope/revocation and issuer admission at the handoff linearization point.

Do not use `VerifiedPrivate` as a created receipt or relabel a read-only open RW. Do not reconstruct authority from a diagnostic path or copy an unrelated DirectoryPlan's proof. Handoff may clone the validated native directory into the derived root, but it must preserve the original revocation chain and independent pin ownership.

Privacy can change later without an identity change. Consequently, handoff checks are not an ongoing privacy promise.

## 6. Sensitive consumer prerequisites

### Actual source gaps to close before promotion

- `settings_device_colors.rs`: `DeviceColors::verify` currently checks ApprovedRoot/path/stat, then uses path-based `NativeSystemFiles::read/replace`. Those checks do not establish current-private directory/leaf policy at use.
- `journal_anchor.rs`: `JournalRoot::open` currently calls `protect_retained_directory`. Promotion needs verify-only current-private admission; automatic repair cannot stand in for existing-root privacy or birth evidence.
- `FileService::authorized(false)` does not run native commit fences. A write-only fence by itself does not cover private reads.

### Proposed narrow private-consumer adapter, for separate API review

Reuse the existing private-leaf/publication machinery rather than adding another persistence implementation. A proposed opaque `PrivateMetadataScope` would be constructed from the exact retained approved-root capability through a Files-owned verify-only guard. No arbitrary File/parent-handle, boolean fence or diagnostic-path constructor is exposed.

```rust
// Proposed, not currently exported or approved:
FileService::private_metadata_scope(self: &Arc<Self>) -> Result<PrivateMetadataScope>
PrivateMetadataScope::read_leaf(&self, leaf, max) -> Result<PrivateRead>
PrivateMetadataScope::prepare_publication(&self, destination, stage, expected)
    -> Result<ScopedPrivatePublication, ScopedPrivatePreparationFailure>
```

The opaque scope retains original authority, actual root/pins, full identity, binding information and a verified private-policy fingerprint. Construction reuses the shared no-follow/native-identity walker; it grants no broader scope. Native-relative roots require their real retained binding, never path reconstruction from a label.

- Reads check current directory privacy/binding before and after bounded private-leaf reads; leaf identity/privacy/version checks remain in the existing helper.
- Stage creation, writes and publication recheck the same guard at each existing native commit boundary. A private guard attached to a derived scope may participate in existing commit fences, but read checks must remain explicit.
- Retain `ScopedPrivatePreparationFailure`/publication custody in the original store/actor before diagnostics or await/cancellation boundaries. Preserve explicitly; never map it to a plain storage error while dropping the attempt.
- DeviceColors uses this guarded descriptor-relative metadata flow, retaining its existing bounds, CAS, deduplication and three-colors behavior. Do not use a fake Go fence or repurpose `native_child_issuer` without actual native acquisition.
- Editor/journal initialization stays on `EditorBackend::new_anchored`. Its journal root becomes verify-only and continues checking current privacy/binding before reads, lease/stage use and every write/commit. Preserve its existing revision/recovery limits; do not silently change the V1 journal to pathless/native-relative recovery.
- Explicit legacy repair, if still required, remains a separately authorized flow and never produces private-birth evidence.

These per-use changes and their shared-lifetime tests are promotion prerequisites, not follow-up cleanup.

## 7. Failure, cancellation and unwind

All creator/Directory/publication typed failures retain the actual original owner/custody. No `From` to FileError/stdError shortcut is introduced. The attempt row is registered before native creation; a panic before an FD is stored remains uncertain possible-artifact evidence, not NoCreate/NoWrite.

The production native directory entry has no await between admission, native capture and returning a committed result/retained failure. Cancellation before execution must not begin creation. Cancellation after admission cannot erase the row: completion/cancellation handling retains the original owner until explicit disposition. Async callers keep their issuer/keeper outside task-local unwind/cancellation scopes; a task-output drop is not disposition.

At injected unwind boundaries, recovery is bookkeeping/preservation only. Store any actual FD/full identity first, seal admission, then propagate the real typed failure or perform explicit stale preservation under the original keeper. Do not unlink by name, reacquire authority, retry ambiguous creation or infer resource cleanup from closing custody.

## 8. Required reviewed test gates

Use real production entrypoints, not only prototype constructors. Tests remain scoped and private; no GUI, Go/runtime or Service-lib test relink is authorized by this plan.

1. Public-entry ownership: real mutation guard/keeper, no arbitrary guard-proof/File/bool constructor, denied RO/scope/revoked authority.
2. Real multi-step missing paths; a prefix created by an earlier sequential plan; all five ordinary default/explicit frontend paths genuinely succeed under the reviewed NTFS threat model.
3. Existing reopen/current privacy, correct RW request/full ID/pin; no ACL repair or created/birth claim. Outside inserted prefixes and FILE_CREATE collisions are rejected without adoption.
4. Bounded capacity failure before native call; no allocation needed to retain post-create failures.
5. Inject failure/cancel/panic before FD capture, after actual FD but before full ID, after ledger commit/before clone, between missing steps, and before final handoff. Retention reports remain truthful and keeper preservation works with poisoned locks.
6. Concurrent same-issuer preparation refusal; preserve racing active entry; terminal admission across all issuer plans/clones; no later step or handoff after preservation wins.
7. Independent derived pins survive release/preservation of another view. Exercise real ancestor/name replacement attempts and verify either native denial or guarded binding failure without redirected reads/writes. No global revoke of unrelated user I/O.
8. Weaken directory privacy without changing identity before read, stage and commit. Sensitive scopes and DeviceColors/journal guards reject each case; birth/handoff success does not mask the change. Test private leaf privacy changes independently.
9. Typed metadata/journal failure and async cancellation retain original publication custody and explicit disposition. No log-only or error-string conversion loses it.
10. Keep strict generic File-based parent refusal tests. Keep alternate spelling/reparse/full-ID tests. State NTFS/Windows prerequisites and all unsupported ReFS, privileged/same-account and Unix/VFS limits explicitly.

Only after plan/API review, these gates and the single coordinated metadata/Clippy window may a production integration/default-policy change be proposed. None of the previous unit or native checkpoints is promotion approval.
