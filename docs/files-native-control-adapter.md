# Native control-root adapter

Status: actual compiled Files adapter and capability-scoped metadata API. Scoped Windows leaf tests pass; Darwin aarch64 has metadata/strict lint checks only. Native consumer integration, default-path private-parent support, Host/VFS confinement and cross-domain atomicity are not accepted by this leaf result.

## Exact API

```rust
trait NativeCommitFence: Send + Sync {
    fn check(&self) -> files::Result<()>;
}

enum PreparedPrivateDirectoryRef<'a> {
    Created(&'a CreatedPrivate),
    Verified(&'a VerifiedPrivate),
}

FileService::native_child_issuer(
    self: &Arc<FileService>,
    parent: PreparedPrivateDirectoryRef<'_>,
    fence: Arc<dyn NativeCommitFence>,
) -> Result<NativeChildIssuer>;

FileService::reopen_native_directory_rw(
    self: &Arc<FileService>,
    original: &VerifiedPrivate,
    fence: Arc<dyn NativeCommitFence>,
) -> Result<VerifiedPrivate>;
```

`NativeChildIssuer` has `created_directory_root(&CreatedPrivate)`, `verified_directory_root(&VerifiedPrivate)` and `parent_root()`. The first two return narrower `Arc<FileService>` capabilities. Only trusted bootstrap owns the issuer; neither it nor the original account grant is exported through IPC, peers or views.

## Actual authority and provenance

The receiver is the ONE bootstrap-private LocalDesktop RW issuer constructed after actual Go acquire/into_acquired, before owned control children. This account authority already exists in trusted native bootstrap. The adapter only narrows it. Peer ApprovedFullAccount, read-only/revoked issuers and unproved selected ApprovedRoot issuers are rejected; no selected-root scope is inferred from a label.

The factory borrows a SEALED original DIRECTORY result, not a File/path, and does not consume its original preparation custody on error. It checks current privacy, binding, full identity, requested RW provenance, no cleanup-started state and the actual Windows pin. Child binding additionally requires the exact direct-parent full identity and original receipt leaf. Derived roots preserve the original access, revocation ancestry and creation ledger.

Requested `PrivateAccess` is recorded by real create/open factories. It is not inferred from Unix directory FD flags. A readonly Verified result cannot be relabelled RW. The explicit reopen method checks the original RW issuer and same acquired fence under mutation discipline, opens relative to the receipt's SAME retained parent/leaf without following links, compares full identity with the original held object, and issues NEW sealed RW provenance. On Windows that reopen requests no DELETE access and denies FILE_SHARE_DELETE. No absolute reopen or raw-FD RW constructor exists.

## Resolution mode

Existing PathBound behavior and ordinary path-root derivation remain unchanged. Native roots use distinct AnchoredRelative resolution:

- Every lookup starts from the SAME held directory. Absolute/rooted/drive/prefix/traversal selectors are rejected portably.
- Results are relative selectors. `diagnostic_label()` is non-authoritative and never reopened or joined as authority.
- `root_path()` is None, but capability kind remains explicit ApprovedRoot, NOT FullAccount.
- Path-oriented DirectoryPlan/cwd APIs return Unsupported until true anchored variants exist.
- V1 user-stage journals, editor staging and inbox recovery reject native-relative scope before lease, `.next`, recovery or publication. They need separately accepted user-stage target and recovery authority contracts. Native metadata capabilities do not authorize cleanup outside their held roots.

## Capability-scoped metadata API

`read_private_leaf(&self, leaf: &OsStr, max: usize) -> Result<PrivateRead>` reads private bytes/version only. It admits AnchoredRelative scope and checks ordinary revocation before and after. Service checks actual ownership separately.

`prepare_private_leaf_publication(self: &Arc<Self>, destination: &OsStr, stage_leaf: &OsStr, expected: Option<&PrivateVersion>) -> Result<ScopedPrivatePublication, ScopedPrivatePreparationFailure>` retains the same Arc and original custody. Its failure box is allocated before native create. Possible creation retains an attempt even if the kernel returned no child FD. No retained attempt means a proven pre-create/no-retained-attempt error from the underlying helper.

The scoped owner exposes bounded one-shot `write_all`, `publish`, `cleanup`, `preserve_stale`, phase and diagnostic/version views. It exposes no raw File, inner owner, parent, path, issuer or Deref. `ScopedPublishedReceipt` is only a borrowed identity/version view, not ownership proof. Write and publish use the same capability/fences under the mutation guard; final publication authorization follows prepublication metadata checks. Cleanup uses the narrow original receipt after revocation; lock poison remains an explicit error, while preservation does not require a lock or new write authority. Unix preservation semantics remain unchanged.

## Single-leaf private directory scope

`FileService::open_or_create_private_child(self: &Arc<Self>, leaf: &OsStr) -> Result<Arc<FileService>, DirectoryPreparationFailure>` narrows a native RW capability to one private direct child. For example, trusted approval storage can derive `remote-v1`, then use the bounded leaf metadata methods for `approvals.json` and `write-pending` within it. A slash-containing selector is not a leaf.

The caller holds the original mutation guard throughout lookup, existing-child verification/RW reopen, creation, postvalidation and derivation. The private `create_private_child_retained(authority, parent, name, &mutation)` helper never reacquires that lock. It returns `Dir` or typed `DirectoryPreparationFailure`, with the original row pre-reserved and the actual created File committed before a fallible clone. Errors keep the same authority Arc and bounded ledger. Keep the preparation owner outside unwind.

Parent and child privacy are verify-only. Existing children first produce sealed RO provenance, followed by explicit scoped RW reopen and full identity equality. Missing children use exclusive native private creation. A collision is not retried or adopted, and no DirectoryPlan, diagnostic path or account-issuer fallback exists. The child inherits all revocations, fences and the original ledger. This does not enable native journals or uploads.

Seven scoped Windows child tests passed, including direct helper completion while the caller already holds the mutation guard, exact held scope and metadata namespace, existing-child actual Windows pin without created ownership, rejected selectors/access/loss without creation, verify-only parent/child privacy, junction collision rejection and post-create loss retaining the original typed keeper. Darwin aarch64 metadata and strict lint checks are compile-only.

## Owner fence and lifetime

Files has no transport dependency. Service/bootstrap implements the opaque callback and owns the exact `ownership.lost()` token, the SAME `ownership.root_lifetime()` allocation and actual native custody. The token is available after real acquire; the callback/issuer is constructed then, before root output/owned children/consumer registration. No pre-Go active issuer or new verifier scope is invented.

Files synchronously checks the callback alongside original revocation before staging and within existing mutation/commit checks. Every derived capability preserves/ANDs inherited restrictions; a later allowing fence cannot replace one that denies. Watcher-driven revoke is supplemental, not the sole fence.

This is a COMMIT restriction, not a NoOwner/cleanup proof. The implementation does not claim atomicity between the final synchronous token sample, a filesystem syscall and Go's ownership domain. Native consumers retain their existing ownership admission checks for reads and lifecycle operations. Keeping a RootLifetimeGuard alone is not proof of every possible namespace/confinement guarantee.

## Ownership and leaf evidence

Walter owns RootResolution/private fields, initialization, base dispatch and fence-preserving derivation. Electra owns the sealed adapter/provenance and explicit relative RW reopen. Tughril owns actual post-acquire prepared-parent/callback/lifetime handoff. Musa migrates storage consumers only after receiving the original retained control children.

Five leaf tests passed: relative-only resolution/label isolation; RO-to-RW requires explicit authorized same-object reopen; bad issuer/kind/parent/revocation rejection; inherited restrictions cannot be weakened; and synchronous mutation denial plus custody retention without a watcher. These are owned current-account source/kernel cases, not Service/Go/GUI/network, another-account, ReFS or native platform acceptance.
