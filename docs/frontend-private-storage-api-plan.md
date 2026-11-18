# DeviceColors and journal private storage — narrow API proposal

Status: metadata direction accepted for **TEST/CANDIDATE only**, before consumer edits or wiring. Proposed APIs below are not compiled exports or competing Directory admission signatures. Journal initialization/activation is separately BLOCKED for BOTH PathBound and Native until accepted actual user-stage authority and V2 schema. Files producers must agree and publish their actual implementation before consumers use it. No public Windows creator route, UI/default policy, compatibility constructor or runtime permission changes here.

## 1. Separate current contracts

- General ApprovedRoot is scope, not continuing privacy.
- Private birth/current handoff does not replace checks at sensitive read, stage, write and commit.
- Current DeviceColors path-compatible storage has scope/binding and CAS checks, but is not a current-private metadata guarantee.
- Current journal admission can protect/repair the retained directory. A new strict-private path must be verify-only rather than silently treating that repair as privacy/birth evidence.
- A current-private guard grants no cleanup ownership. Cleanup remains based on actual owned native/publication receipts and the original retained owner.

## 2. Shared Files-owned current-private guard

Introduce one internal `PrivateRootCheck` for candidate metadata and read-only journal verification; reuse in an activated journal remains deferred behind Stage 2. Its constructor is private to Files. It receives the original capability and an actual retained binding, not an arbitrary File, path-derived regrant, bool or caller-made proof.

It retains:

- original scope/access/revocation and existing native restrictions;
- the actual root handle/full identity and independent native pins;
- the original no-follow binding/parent/leaf information needed to check that exact root;
- current verified private directory policy/fingerprint, not a historical birth certificate.

For path-bound capabilities, use the shared bounded no-follow walker to validate the original spelling and compare its retained object against the actual capability root. Do not canonicalize an alias into authority. For native-relative capabilities, require the real sealed binding retained by the native adapter; do not reconstruct it from a diagnostic label. If that binding is unavailable, return Unsupported.

`check(Read)` checks authority, current root privacy and binding before/after a sensitive read. `check(Stage/Write/Commit)` also checks RW and all existing native commit restrictions at the corresponding native boundary. It never repairs ACLs or relabels RO. A write-only NativeCommitFence is insufficient for reads: read checks are explicit. Avoid an Arc cycle by retaining the pre-derivation authority/binding rather than storing the newly derived scope inside its own fence.

Generic user file/folder APIs and general ApprovedRoot semantics remain unchanged. No new Go/owner/elevation authority is inferred.

## 3. Exact proposed metadata surface

Add a narrow opaque adapter over the EXISTING private-leaf/publication implementation:

```rust
// Proposed, not yet exported/compiled:
pub struct PrivateMetadataScope; // no raw parent/inner capability accessors

impl FileService {
    pub fn verify_private_metadata_scope(self: &Arc<Self>)
        -> Result<PrivateMetadataScope>;
}
impl PrivateMetadataScope {
    pub fn check_current(&self) -> Result<()>;
    pub fn read_optional_leaf(&self, leaf: &OsStr, max: usize)
        -> Result<Option<PrivateRead>>;
    pub fn prepare_leaf_publication(
        self: &Arc<Self>,
        destination: &OsStr,
        stage: &OsStr,
        expected: Option<&PrivateVersion>,
    ) -> Result<ScopedPrivatePublication, ScopedPrivatePreparationFailure>;
}
```

The constructor is read-only/current verification, creates nothing and does not change permissions. It narrows existing authority; RW operations recheck actual RW rather than upgrading an RO scope.

Reuse `PrivateRead`, `PrivateVersion`, `ScopedPrivatePublication`, its actual publish outcomes, owned cleanup receipts, and typed preparation failure. Do not add another encoder, CAS writer, SystemFiles implementation, metadata registry or path-based native wrapper.

Optional read maps only a proven absent leaf to None: check the retained private root/binding before and after the attempt. Root/ancestor loss, an unsupported private check, a directory in place of the leaf, permissions and uncertain reads must not become "missing/default colors."

The returned publication carries the shared private-root check through existing scoped write and publish callbacks. Check directory AND leaf privacy/binding, plus expected revision, before preparation and at every write/final-publication boundary. A valid handoff earlier is not enough.

### Original publication ownership before native work

The scope/store allocates its fixed single active-attempt retention slot before the native call, under the existing per-store serialization gate. Its original owner remains outside internal capture/unwind and async cancellation boundaries.

The private implementation must capture into that original preallocated slot; a local temporary box that can unwind before returning is insufficient. The scoped publication/control view and typed failure refer to the SAME retained attempt, not a second owner or reopened authority. No post-create allocation is required to retain it. This is a required internal prepare-into-owner/capture hook for the existing publication implementation, not another public creator/proof API.

On returned failure, store the whole typed `ScopedPrivatePreparationFailure` or its actual retained attempt FIRST, then report borrowed diagnostics. On panic/cancel, the original slot is still reachable for conservative verification/explicit preservation. An unresolved attempt blocks the next save; no replacement of the slot, retry under a fresh stage name, TTL eviction or log-only drop.

Write/publish refusal and durability uncertainty remain distinct. A returned failure does not imply that no file was created or no publication occurred. Owned cleanup and explicit stale preservation keep their current receipt semantics and cannot be authorized by `PrivateRootCheck` alone.

## 4. DeviceColors — additive compatibility and migration

Proposed additional constructor:

```rust
DeviceColors::native_private(scope: Arc<PrivateMetadataScope>) -> anyhow::Result<Self>
```

- Keep `native_anchored(directory, proof)` and its path-compatible/mock-SystemFiles behavior unchanged. Do not silently upgrade its guarantee or route every generic caller to the new private policy.
- New private constructor retains the exact supplied scope. It uses the same fixed leaf `shellfox-device-colors-v1.txt`, existing 1024-byte decode bound, format, color validation, deduplication and last-three-per-kind behavior.
- Share encode/decode/color-update logic. Keep backend-specific revision evidence distinct internally rather than fabricating a SystemFile revision from a path or discarding native private versions.
- The existing per-store gate encloses read/update/stage/commit and original publication retention. Allocate the attempt slot during store construction, not after a creator returns.
- Preserve CAS semantics with tests for concurrent edits, same-bytes rewrites and metadata/privacy changes. Do not silently weaken the current revision contract merely because PrivateVersion is available. If existing PrivateVersion is insufficient for the required comparison, agree an additive native revision representation before migration.
- No-op color updates still perform current-private checks and do not create a stage. Missing data is default only when absence is proved under a valid private root.

Candidate/test wiring may construct this scope from owned already-private fixtures under the existing strict creator. Frontend production continues using its current constructor until the narrow API, private checks, retention and independent review pass. Do not fake Go/native-child-issuer fences to construct the scope.

## 5. Journal Stage 2 — BLOCKED for BOTH PathBound and Native

The metadata adapter direction does NOT authorize a private journal initializer. A private metadata root is neither user-stage recovery authority nor a valid historical V1 identity, whether its capability is PathBound or Native/anchored-relative. General scope, current privacy, actual private birth and no-delete pins do not supply the missing historical authority.

**Native journals, editor and uploads remain OFF.** Do not activate V1 destructive recovery, V1 rewrite/persist, `.next` cleanup, or lease fallback merely because a root was privately created or currently verified. Do not route a Native request through a PathBound/legacy constructor to bypass this blocker. Accepted actual user-stage authority AND an accepted V2 schema are prerequisites for any strict journal initializer/activation.

Only the shared `PrivateRootCheck` and read-only verify-only admission may be candidate/test-staged now. They may inspect current privacy/binding and retain the actual root, but must not acquire a lease, load/recover/rewrite V1, clear a stale `.next`, initialize a registry/editor/upload subsystem, or infer a cleanup receipt. No initializer/activation API is approved by this proposal.

The earlier proposed `new_private_anchored` / `initialize_private_journal` and journal-preparation failure envelope are DEFERRED, not candidate exports to implement now. Their exact signatures and retention protocol must be reviewed together with actual user-stage authority and V2, rather than treating a verified metadata root as sufficient input.

Preserve existing path-compatible constructors/implementations for their separately authorized legacy uses/tests; do not silently break or relabel them. Preserving compatibility does NOT authorize the Native frontend's PathBound journal route. Existing Native-relative `require_path_resolution` rejection alone does not close the PathBound activation gap.

Before future activation, the accepted Stage 2 design must establish actual scope/identity/ownership for recorded USER stages, a reviewed V2 representation and migration policy, real retained lease/publication/recovery custody, and per-use root/leaf privacy+binding checks. It must explicitly decide historical V1 handling without destructive inference, pathname/diagnostic-label regrant, inferred stage ownership, repair-as-evidence, fresh lease fallback or a second recovery engine.

Retain the current privacy/owned-cleanup separation: a directory check is not an owned stage/lease receipt. Failed preparation, malformed history, cancellation, unwind and uncertain publication must keep their original actor/registry custody before diagnostics and explicit disposition. Candidate metadata tests do not establish any of these journal activation gates.

## 6. Candidate tests and decision gates

Before any consumer/default wiring decision:

1. Private scope construction is verify-only: a broad/changed ACL, wrong root, alias, missing binding or RO write attempt rejects without repair or outside mutation. Existing compatibility constructors retain their exact prior behavior.
2. Same root ID but weakened privacy is rejected separately at read, prepare, write and commit. Test leaf privacy independently. Read-only authorizations do not accidentally skip the guard.
3. Root/ancestor replacement either fails natively under independent pins or fails binding checks without redirected reads/writes. Release original plan/ledger view and retain the consumer: its actual independent pin/guard must remain valid.
4. Native preparation fault/panic before FD capture, after returned FD and before full ID/postchecks, partial write, publish refusal/uncertainty and async cancellation retain the original scoped attempt. Poison recovery is preservation-only, never authority recovery.
5. DeviceColors preserves format/bounds/defaulting, three-color behavior, concurrency/CAS and no-op behavior. An invalid/private-unavailable store cannot masquerade as empty defaults.
6. BOTH PathBound and Native negative Stage 2 tests reject before lease acquisition/fallback, V1 destructive recovery/rewrite, `.next` cleanup, registry/editor/upload initialization or activation. A valid private metadata root/current guard must not pass that gate. Read-only verification never repairs. Positive journal lifecycle/recovery tests await accepted actual user-stage authority and V2; legacy compatibility tests are not Native activation approval.
7. Owned cleanup receipts remain separate from current privacy guards; failures and stale evidence are not NoWrite/NoOwner proofs.
8. Directory admission preservation does not globally revoke the existing metadata/user-I/O scope. Private current checks and owner commit fences still apply after another view releases its pins.

Keep Windows/NTFS prerequisites, unsupported ReFS behavior, admin/SYSTEM/hostile-same-account and Unix/global-VFS limits explicit. No Service-lib/Go/network/GUI/runtime permission is added. DeviceColors still requires the outstanding PrivateVersion/CAS/retention review, exact producer exports, real privacy/shared-lifecycle tests and independent review before any consumer wire. Metadata approval does not remove the independent BOTH-PathBound-and-Native Stage 2 journal/editor/upload blocker.
