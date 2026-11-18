# Original publication owner and control view

Status: PROPOSAL ONLY. No implementation, producer export or Endpoint/DeviceColors wiring is approved by this document. Existing by-value APIs remain unchanged. Endpoint is one-shot first; DeviceColors reuse, retirement, PrivateVersion/CAS and cleanup policy require separate review.

## Mandatory outer keeper dependency

Use the SAME owned [Native Boot keeper and common driver proposal](native-boot-keeper-proposal.md). The canonical transport signature/proof input is [Mulavarman's producer input](../tmp/mulavarman-native-producer-input.md). Tugh's separate transport input is Service requirements/supporting input, not another producer API. This publication proposal owns only the Files publication/adapter contract; it maintains no transport API table or Driver design. Do not treat an owner field inside the current serve future as sufficient.

That proposal's outer placement, consuming-transition gate and future-drop contract are mandatory. The native driver retains the actual original guard/controller/root and publication owner outside executor block_on and every borrowed start/prepare/cleanup future, catch and diagnostic boundary. Mulavarman's independent actual-source audit of consuming transport transitions remains a gate. No new Host or reconstructed cleanup scope is allowed.

The API below addresses publication capture only. It cannot establish that its caller retained the original keeper correctly. Endpoint integration remains blocked until both contracts are accepted and implemented together. Panic, caller-future drop or channel failure is retained unknown custody, never a generic catch-to-clean/NoOwner conversion.

## Independent transport audit dependency

Mulavarman's actual-source audit adds separate blockers. The outer keeper is necessary but not sufficient:

- Synchronous `bind_root(self)` and `into_acquired(self)` moves are not async gaps merely because they consume a value. The concrete Service `mem::replace(..., Transferred)` path moves actual cleanup custody into locals. It needs a reviewed borrowed in-place transition or a phase-independent cleanup-control view of the SAME original owner before that movement.
- `OfflinePreparation::acquire(&mut self)` already registers its ORIGINAL child before pipes and await. Cancellation requests release; it does not establish exit or NoOwner.
- The compatibility `HostOwnership::acquire` path is unsuitable for this driver contract because it recaptures paths and may expose only an error after resource failure. It is not a substitute for original retained preparation custody.
- NetworkHelper `Process::spawn` registers PendingHelper only after pipes/tasks. Its post-spawn return gap is a separate gate. The existing remote static gate remains before initialization; this proposal cannot enable that route.
- `RootLifetimeGuard` retains directory lifetime only. It is neither process ownership nor cleanup authority.

No driver implementation against the consuming APIs is approved by this proposal. The transport control-view/in-place transition contract must be reviewed against actual source, not inferred from a panic catch or this publication API.

## Additive proposed API

These names are proposed, NOT compiled exports:

```rust
pub struct PrivatePublicationOwner;   // original persistent fixed slot, not Clone
pub struct PrivatePublicationControl; // opaque SAME-slot control view, not Clone
pub struct PrivatePublicationAttemptId; // opaque identity, no public constructor
pub struct PrivatePublicationSnapshot;  // owned diagnostic data, no authority
pub struct RetainedPrivatePreparationFailure;

impl PrivatePublicationOwner {
    pub fn retained(scope: Arc<PrivateMetadataScope>) -> Self; // immutable binding, no FS/admission
    pub fn snapshot(&self) -> Result<PrivatePublicationSnapshot>;
    pub fn verify_retained(&self) -> Result<()>;
    pub fn cleanup_current(&self) -> Result<Option<PrivateCleanupDisposition>>;
    pub fn preserve_current(&self) -> Result<Option<PrivatePreservationState>>;
}
impl PrivateMetadataScope {
    pub fn prepare_leaf_publication_into(
        self: &Arc<Self>,
        owner: &PrivatePublicationOwner,
        destination: &OsStr,
        stage: &OsStr,
        expected: Option<&PrivateVersion>,
    ) -> Result<PrivatePublicationControl, RetainedPrivatePreparationFailure>;
}
impl PrivatePublicationControl {
    pub fn snapshot(&self) -> Result<PrivatePublicationSnapshot>;
    pub fn write_all(&mut self, bytes: &[u8]) -> Result<()>;
    pub fn publish(&mut self) -> PrivatePublishOutcome;
    pub fn cleanup(&mut self) -> Result<PrivateCleanupDisposition>;
    pub fn preserve_stale(&mut self) -> Result<PrivatePreservationState>;
}
impl RetainedPrivatePreparationFailure {
    pub fn error(&self) -> &FileError;
    pub fn snapshot(&self) -> &PrivatePublicationSnapshot;
    pub fn control(&self) -> Option<&PrivatePublicationControl>;
}
```

The failure does not implement std::error::Error or implicit From conversions. Its error borrow refers to its OWN typed diagnostic snapshot, copied only AFTER original custody is retained, not to unlocked mutex state. Failure to allocate/format a snapshot leaves the original attempt rooted in its owner.

The control view retains a reference to the same slot and attempt identity. It never owns a replacement attempt, clones scope authority, exposes a File/parent/inner reference, or supplies a public proof constructor. Snapshots contain bounded phases, errors, identity/version diagnostics and capture/disposition facts only. PrivateVersion remains diagnostic/CAS evidence, not cleanup ownership.

## Original owner before native entry

The stable outer Boot/Store keeper reserves the owner location before startup. After the real scope exists, it constructs and stores PrivatePublicationOwner before creator entry. The owner has an immutable reference to that SAME scope outside its mutable slot; it cannot be rebound. This permits entering native admission before locking the slot, without slot-to-scope inversion. Merely setting attempted=true with an Option still empty is not capture.

Internal prepare_into_original_owner must:

1. Validate inputs, scope, current privacy/binding, RW and existing restrictions before effects.
2. Allocate request data and native storage, then install the actual OwnedPrivatePublication plus raw CreationCustody inside the ORIGINAL fixed slot.
3. Record native-entry uncertainty before entering the creator.
4. Pass the native out-parameter directly to that slot's CreationCustody.file. The returned optional FD is rooted there BEFORE full-ID queries, privacy/binding checks, cancellation observation or diagnostics.
5. On successful verification, move the same File/full ID from capture fields to stage fields WITHIN that same attempt. This transition is infallible and has no allocation, callback, snapshot or fallible window between take and replacement.

Do not call a by-value prepare and move the result back. A local Box that can unwind before returning is insufficient. The slot remains reachable on all errors and unwind even if no control view was returned.

The proposed scope entry delegates to the SAME existing private publication state machine, bounded writer and CAS implementation. They are not another SystemFiles implementation or parallel writer. Generic File-based creator policy remains strict. Native-relative scopes without real sealed binding remain Unsupported; no label reconstruction or Root-field addition is implied here.

## Attempt admission and one-shot first version

The first candidate owner supports ONE attempt for its lifetime. No reset, replacement, TTL eviction or automatic rollover API is proposed.

- Every mutating view validates its owner/attempt identity and phase under the slot lock.
- Existing bounded write remains one-shot; partial/uncertain work is never replayed.
- An occupied, unresolved, interrupted, cleanup-sealed or preserved owner refuses another prepare.
- Cleanup and preserve seal ATTEMPT admission before acting. This is not global FileService/server revocation.
- A stale or terminal control view can inspect diagnostic state but cannot write or publish.
- An attempted native operation with no child FD still retains possible-artifact evidence. It is not NoOwnedAttempt or proof of absence.
- Option::None from cleanup/preserve is permitted only for structured, proven no-native-entry state. It must never stand in for interrupted capture.

Endpoint's existing one-shot policy fits this first version. DeviceColors needs an independently reviewed known-result retirement/reuse contract before repeated saves can use it. Known visibility, durability uncertainty, ownership and artifact disposition remain distinct. A fresh read/version is never used to manufacture ownership or clear uncertainty.

## Lock order and callback limits

All attempt mutators, including prepare/write/publish/cleanup/preserve, acquire:

```text
existing outer Store/Boot serialization
    -> Files global MUTATIONS
    -> original publication slot
```

No slot-to-MUTATIONS route is allowed. Snapshot takes only the slot, copies owned diagnostic data, and invokes neither commit fences nor arbitrary visitors/callbacks. Caller diagnostics/progress/logging run outside protected slot and owner boundaries.

The final native write/publication checks use only the reviewed Files-owned direct-FD current-root/binding checks and existing narrow commit restrictions, never a caller-supplied callback. Their exact placement and non-reentrancy must be reviewed with the producer. In particular, a held OfflineScope admission must not invoke that scope's spawn path, Go permission verifier, helper request or recursive enter. Original-controller rechecks must be leaf-only state checks. An incompatible existing restriction cannot be silently skipped or moved into a boolean fallback; it blocks wiring until its real call graph is reconciled. No unsafe inner-reference escape or detached writer is an alternative to this lock contract.

A poisoned normal mutation/slot lock is a typed failure, not ordinary authority recovery. Conservative snapshot/inspection may recover only to observe retained evidence. Cleanup does not clear poison or bypass a failed mutation lock. Preservation remains a narrow owned-resource disposition, with explicit failure if the required lock cannot be acquired; it never recovers normal write authority.

## Pending Files capability-to-transport permit adapter

This is E's alternative for JOINT REVIEW, not a second implementation mandate. W/Mul compare it with the canonical transport input's candidate NativeCommitFence::enter adapter and publish ONE selected contract before implementation. This section maintains no transport producer API.

This section is PROPOSAL ONLY, not another transport producer contract. It consumes the canonical Mulavarman input linked above. Current `NativeCommitFence::check() -> Result<()>` is a health/restriction sample. It returns before the native operation and cannot hold or drain a native admission permit. It must not be presented as serialized seal/commit protection.

Proposed additive Files adapter interfaces, not compiled exports:

```rust
pub trait NativeOperationFence: Send + Sync {
    fn enter(&self) -> Result<Box<dyn NativeOperationPermit + '_>>;
}
pub trait NativeOperationPermit {
    fn check_original_owner(&self) -> Result<()>;
}
```

The concrete Service adapter retains the canonical producer's owned fence, SAME scope/original controller and SAME Arc-backed Bound/root retention. `enter` wraps the canonical producer's actual borrowed admission. The resulting permit is synchronous, non-Send, non-Clone and cannot cross await or be stored self-referentially in the publication owner. Its drop ends the bounded admitted native segment, not ownership or cleanup.

There is NO default implementation using lost/check/alive/root-lifetime observations. If real producer admission or sealed root binding is unavailable, the guarded path is Unsupported. Implementing the trait alone does not create scope or authority; guarded construction remains tied to the original producer-established native scope.

### Guarded capability boundary

Prefer an opaque native metadata/control adapter rather than adding unreviewed FileService Root fields. It retains the real sealed binding, private inner FileService, current-private check and one immutable canonical native fence Arc. It exposes NO inner FileService Arc/File/path/account issuer. Every child and retained control view keeps that SAME native fence source and existing restrictions.

The first candidate supports one native admission domain per guarded lineage. Reuse the canonical Arc once; reject a different native domain rather than replacing it, skipping a restriction or recursively acquiring the same scope. ALL existing restriction and revocation checks remain ANDed, including the current inherited-vector max32 bounds. No old restriction may disappear because one real permit was acquired. Broader multi-domain ordering would need a separate contract.

The actual audit must inventory ALL NativeControlChild.files(), derived/cloned Arc<FileService>, by-value/compatibility and private helper entry escapes. Every accepted native path must either enter its inherited real permit or be unreachable behind the opaque adapter. Existing raw Arcs cannot remain an alternate route from the new Driver. No compatibility or unsupported-permit fallback is permitted.

The current NativeControlChild.files()/raw-Arc consumer route does not acquire this guarantee merely by being wrapped once at construction. The guarded consumer batch must remove that escape from accepted native paths. Existing by-value/general Files APIs remain compatibility scope operations, not a claim of native seal/commit coverage.

### Constructor and descendant correlation gate

The guarded constructor must not accept independently paired raw FileService, fence and File/path inputs. It must be private to the original bootstrap binding path. Its real source dataflow must establish all of the following before producing a scope:

- The fence comes from the SAME installed acquired preparation/original controller. A copied lost token or equal-looking identity is not scope correlation.
- The canonical producer retains the SAME Arc-backed Bound value originally installed through borrowed root binding, not a newly captured path or another Bound allocation.
- The Files root binding comes from the original sealed prepared directory and retained parent/leaf/full-ID proof. Logical RW and the actual requested/pinned provenance are checked separately; RO is explicitly reopened, not relabelled.
- The guarded scope's actual root ID matches that binding. For a child, the sealed receipt's direct-parent ID must equal the parent scope's held root ID. Existing reopen compares the new native handle to the still-held original.
- Every descendant carries the EXACT SAME immutable adapter Arc and retains original access/revocation/AND restrictions/max32 bounds. Current private state and binding are rechecked at use; provenance alone is insufficient.

Current source has no such guarded constructor. NativeControlFiles::new stores lost/lifetime/creation retention; NativeControlChild.files exposes a raw Arc. BoundOwnershipRoot exposes an opaque identity but its Arc-backed allocation and native file are private. Therefore construction must use producer-established same-root dataflow or an explicitly reviewed producer correlation operation. Do not compare formatted identity strings, re-open a path, extract a new root, or assume that arbitrary same-ID arguments belong to the same scope. Any needed producer correlation API belongs in Mul's canonical input, not a competing table here.

The actual escape inventory includes NativeControlChild.files calls in endpoint publication, remote setup and control revoke, downstream NativePreferenceStore/PrivateApprovalStore raw capabilities, issuer parent_root/created/verified/reopen paths, private-child Arc results and legacy ScopedPrivatePublication mutators. New Driver paths must neither issue nor reuse those old escapes as a fallback. Existing compatibility code remains outside the proposed guarded guarantee until a coherent migration removes that route from native consumers.

### Exact lifetime through native work

For prepare/read/write/publish and native directory operations:

1. The original outer keeper and publication slot already exist as described above.
2. The public guarded adapter entry borrows its immutable native fence and enters real native admission ONCE, before directory admission/MUTATIONS/slot locks.
3. Private workers receive a borrowed context referencing that actual permit. They do not call enter again or mint a guard-proof constructor.
4. The actual permit remains alive through the bounded synchronous operation, original FD/state capture, native syscall and immediate outcome registration. No await or arbitrary diagnostics/user callback runs in that segment.
5. At the native boundary, current private-root/leaf/binding/CAS checks precede the final original-controller recheck under the already-held permit. That recheck must not reacquire scope admission. The established original-controller lock direction is preserved.
6. Release slot and Files guards before releasing the native permit. Diagnostics and later await occur only after the retained state/outcome is recorded.

New directory operations enter native admission before calling the existing DirectoryAdmissionGuard acquisition that claims the directory domain and owns MUTATIONS. E must not reacquire its mutation mutex. Read/reopen operations do not thereby become evidence of filesystem creation.

### Lock order and seal/drain

The proposed nested order is:

```text
outer per-store serialization, if any
  -> original native scope admission permit
  -> directory-operation claim, when required
  -> Files MUTATIONS
  -> publication slot
  -> brief original-controller check under the held scope permit
```

The initial enter can inspect the original controller under scope admission, then release the controller lock before waiting for Files locks. There must be no original-control-to-scope or slot-to-scope/MUTATIONS path. The concrete producer/adapter lock paths need review against actual source; a trait signature alone does not prove them.

Transport scope seal is invoked with NO Files MUTATIONS, publication slot, original-controller or store/diagnostic lock held. It waits for previously admitted synchronous segments, permanently closes admission, then releases its gate. It cannot be invoked recursively from an admitted operation or its fence callback. After seal returns, old and newly issued guarded views cannot admit new native work, even if their lost token is unchanged.

Only after that drain may the original cleanup controller drive its reviewed release/wait protocol. No control lock is held while waiting for scope seal, and no scope guard is held across cleanup awaits. Native child exit/NoOwner evidence remains the canonical producer's separate obligation.

Attempt cleanup/preservation is different. It seals ATTEMPT admission and uses only original owned receipts under MUTATIONS -> slot. It does NOT enter a sealed native-use gate, grant a fresh write or globally revoke unrelated Files/server I/O. A pending guarded writer that later obtains the slot must observe the attempt seal and refuse before effect. Snapshot remains slot-only and calls no scope, MUTATIONS or commit callback.

A stuck native syscall can delay draining; timeout or poison is not successful seal/cleanup evidence. Interrupted native work keeps unknown artifact custody. No poison clear/recovery into ordinary authority is proposed.

### Confirmed Service/verifier reentry blockers

Tugh's actual-source audit confirms these current paths are NOT safe under a held native admission permit:

- `security.rs:126-176` calls scoped `PermissionVerifier::check/protect`.
- Windows permissions `may_protect` takes the same scope lock, and `spawn_in` reaches scope spawn/ledger locking.
- Unix permissions reaches `scope.native`, also taking that same lock.

Registered asynchronous verifier admissions must stay outside Files forward-use permits, MUTATIONS and publication slots. Never wrap a scoped Go verifier/protect/check in NativeAdmission. This is not permission to omit those checks: preserve their required admission/retention before the direct native segment, then perform current direct-FD privacy and binding checks inside it. Acquired protection checks already holding the ledger lock need a leaf state predicate, not a nested may_protect call.

Permit recheck must inspect only cached ORIGINAL controller identity/holding/release/known-exit/fault/lost state through the established leaf lock direction. It must not call poll_exit, try_wait, release, wait_exit, a permission helper, scope spawn, FileService::authorized recursively, or any I/O/diagnostic/user callback. Cached health is a restriction and cannot be upgraded into positive exit/NoOwner proof.

Current endpoint slot -> SDK MUTATIONS, native_host_private.with_created birth-slot -> factory MUTATIONS, and bind_control children-mutex -> factory paths are additional concrete blockers. A drop-in enter around those calls creates the wrong order. They require reviewed original borrowed state and private under-permit/non-reacquiring operation contexts. No moving a sole receipt to a local, reconstructed root, new public guard-proof constructor or arbitrary callback under the slot is approved as a workaround.

The complete Service escape inventory and constructor/descendant correlation review remain pending. Do not implement the adapter or claim that these three examples complete that inventory.

### Guarantee boundary

The permit supplies internal admission/seal ordering. It does not freeze external process exit, administrator/same-account actions, filesystem ACL changes or Go's ownership domain. The final controller check remains a sample of that predicate. An external exit immediately after it is not made atomic with a filesystem syscall. NoOwnerProof, publication durability, artifact absence and native capability admission remain separate facts.

The adapter API, immutable native binding/factory and removal of raw-Arc escapes require joint producer/Service/Files review before implementation. They are not approved by the transport direction alone.

## Existing writer and privacy/CAS rules

Keep current PrivateRead/PrivateVersion, destination expectation, actual stage/full identity and byte validation, no-follow binding, one-shot write rules and PrivatePublishOutcome distinctions. Recheck SAME original scope and current root/leaf privacy at the read/stage/write/commit boundaries. Keep the final synchronous native fence sample; do not claim Go/filesystem atomicity.

Cleanup uses only original created/publication receipts, including after capability revocation. It cannot derive deletion authority from PrivateRootCheck or a fresh version. Unix preservation without a real accepted fence remains preservation, not check-then-unlink.

This proposal does not resolve PrivateVersion same-bytes rewrite/CAS adequacy or DeviceColors retirement policy. Neither is waived by an owner/control-view adapter.

## Endpoint and report migration gate

After approval, the outer keeper stores the actual owner before borrowed endpoint preparation, and BootGuard uses only its control view. The Endpoint report snapshots that same owner. It does not discover ownership after prepare returns.

The current legacy report case attempted=true plus slot=None is CustodyUnknown, not NoOwnedAttempt or no artifact. That conservative correction does not prove the capture gap fixed. Returned-error retention and compile checks do not prove unwind/future-drop safety.

No Service consumer writer API, common-driver implementation, public metadata export or runtime acceptance is authorized yet. Keep normal binary/DLL, default policy, native journal/editor/upload gates and Go/network/GUI permissions unchanged.

## Current narrow evidence, not promotion

The current Windows candidate/prototype set has 16 tests, not 32 independent tests because it was rerun serially and in parallel. The cap-metadata subtree has 19 passing parent tests plus its exact isolated poison child. Darwin results are compile/lint only. The first timed-out run remains FAILED, and its missing registered-PID exit proof is recorded in tmp/electra-files-candidate-timeout.log.

The staged RootCheck Stage/Write/Commit labels are checkpoint tests, not integration with actual mutators. Native missing-binding Unsupported is a source branch, not an executed negative test. Full-ID query failure and partial/ambiguous native-return capture branches have source retention paths but were not separately injected. The candidate has no real caller cancellation input: injected hook errors/panic and serialized revoke are not general startup cancellation evidence.

Those results do not prove this original-owner hook, native permit adapter, Endpoint boundary or a DeviceColors writer. Actual startup cancellation needs the later reviewed Driver denial context. Do not add an unwired token or fake native fixture to inflate that coverage.

## Required review evidence before wiring

- The ACTUAL outer keeper outlives start/prepare/cleanup future construction, polling, unwind and drop; consuming transitions retain original cleanup control.
- Faults before native entry, after native return before ID capture, after ID capture, partial write, rename uncertainty and cancellation leave the SAME slot/FD/evidence reachable.
- Failure snapshot allocation/formatting failure cannot drop original custody.
- Attempt sealing, stale-view rejection and occupied-owner refusal occur before new native effects.
- Lock-order tests cover cleanup/preserve, snapshot and fence non-reentrancy without global poison/clear.
- Privacy/binding loss blocks private operations without repair; cleanup remains separately receipt-scoped.
- Endpoint one-shot behavior and truthful unknown reports are tested without inferring process NoOwner or artifact absence.

Use scoped owned fixtures and approved test artifacts only. Independent source review, native prerequisites and cancellation/lifecycle evidence remain required. No admin/SYSTEM, hostile same-account, Unix/global VFS, ReFS, Service-lib, Go, network or GUI acceptance is implied.
