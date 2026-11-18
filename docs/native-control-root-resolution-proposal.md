# Native control-root resolution — exact private mode proposal

Status: Files adapter/private hooks compiled; resolution/fence mechanics checked. Electra owns the sealed adapter/provenance API, Walter owns service.rs fields/init/base dispatch, Tughril owns native construction/fence handoff. This is not Host/VFS approval or new runtime filesystem permission.

## One authoritative adapter API (Electra)

```rust
pub enum PreparedPrivateDirectoryRef<'a> {
    Created(&'a CreatedPrivate),
    Verified(&'a VerifiedPrivate),
}
pub trait NativeCommitFence: Send + Sync {
    fn check(&self) -> Result<()>;
}
impl FileService {
    pub fn native_child_issuer(
        self: &Arc<Self>,
        parent: PreparedPrivateDirectoryRef<'_>,
        fence: Arc<dyn NativeCommitFence>,
    ) -> Result<NativeChildIssuer>;

    pub fn reopen_native_directory_rw(
        self: &Arc<Self>,
        original: &VerifiedPrivate,
        fence: Arc<dyn NativeCommitFence>,
    ) -> Result<VerifiedPrivate>;
}
impl NativeChildIssuer {
    pub fn created_directory_root(&self, child: &CreatedPrivate) -> Result<Arc<FileService>>;
    pub fn verified_directory_root(&self, child: &VerifiedPrivate) -> Result<Arc<FileService>>;
    pub fn parent_root(&self) -> Arc<FileService>;
}
```

This is ONE factory contract, not an alternative ApprovedRoot-only overload. The receiver is the once-created bootstrap-private LocalDesktop RW issuer after actual acquisition. The factory receives and validates the actual sealed RW prepared parent, holds that same native object/full ID, and installs the restricting fence before any child capability output. PreparedRef contains only sealed record borrows; wrapping a wrong-kind or RO record does not make it valid. Reject ApprovedFullAccount, RO, revoked and unproved selected-root issuers.

The separately named RW reopen operates relative to the ORIGINAL sealed parent/leaf, under that original native RW issuer and fence. It must return NEW sealed requested-RW provenance only after full-ID equality/binding/privacy checks against the original held receipt. It cannot relabel the borrowed input or mint from an arbitrary File/path. Inputs stay borrowed so failure does not dispose of their native owner.

ChildIssuer validates a direct child's recorded parent ID/leaf, kind/private/RW provenance, full identity/current binding and live fence. It exposes no account-wide issuer or raw writer. The adapter supplies explicit diagnostic labels to the private hook; they never authorize lookup. These are the actual compiled companion names; no second factory is permitted.

## Implemented private fields/hooks (Walter)

```rust
struct Root {
    dir: Dir,
    resolution: RootResolution,
}
enum RootResolution {
    PathBound { path: PathBuf },
    AnchoredRelative { diagnostic_label: String },
}
```

Actual crate-private hooks are `derive_anchored_relative(dir, diagnostic_label: String, fence) -> Result<FileService>`, `retained_anchored_directory() -> Result<Dir>`, `is_anchored_relative()`, `diagnostic_label()` and `require_path_resolution() -> Result<()>`. No placeholders or raw public constructor are exposed.

The enum prevents storing a diagnostic label in an authority path field. There is no public constructor for Root/RootResolution and no public raw-FD RW constructor. Electra's sealed adapter is the only intended native anchored-root construction path; its exact issuer/provenance handoff is the single API above.

FileService retains its existing kind, Access, own/inherited revocation and SAME shared creation ledger. Private field: `native_commit_fences: Vec<Arc<dyn NativeCommitFence>>`. Existing constructors use an empty chain. Derivation retains/ANDs ALL existing restrictions and appends a different new fence (same-Arc deduplicated), bounded at32; a later permissive callback never replaces an older denial. `authorized(true)` checks existing access/revocation first, then the synchronous fence at existing pre-stage/final commit calls. The callback can only deny existing RW authority; returning Ok cannot upgrade RO/revoked scope. It is inherited and never used as cleanup/NoOwner proof.

## PathBound behavior stays unchanged

- Existing approved_root and ordinary derive_root keep PathBound.
- Account capabilities remain their current root=None path behavior.
- Existing absolute-selector strip-prefix, current path binding revalidation and no-follow descendant lookup remain unchanged for PathBound.
- Existing root_path() returns Some(path) for PathBound as before.

The native adapter must not call ordinary derive_root with a diagnostic string and thereby manufacture PathBound authority. Use a private shared inheritance builder or distinct private native derivation after all sealed checks, preserving the same Access/revocation/ledger rules. Any normal derivation entry invoked on an anchored-native root must not silently downgrade it into path-root mode.

## AnchoredRelative behavior

Before lookup, enforce a canonical relative selector:

- Reject absolute, rooted, drive-relative and drive/prefix/UNC/device selectors on ALL supported OS.
- Reject parent traversal and platform-ambiguous backslash/colon selector spellings; do not normalize them into authority.
- Use validated normal relative components only (and an explicitly supported root selector for root reads, if the narrow consumer needs it).

`base()` clones the SAME retained root.dir and walks relative components no-follow. It does NOT call open_absolute or reopen any configured label. Revalidation uses held native identity/binding/provenance, not a diagnostic pathname. No caller-controlled path can change the root.

Existing Target.absolute/output-path plumbing must not receive the diagnostic label. For this mode outputs are validated relative selectors and are never advertised as native absolute paths. Add the crate-private `FileService::require_path_resolution() -> Result<()>` guard: reject AnchoredRelative with Unsupported and preserve existing PathBound/account behavior. Invoke it at path-oriented DirectoryPlan and cwd/home/directory prompt/completion entrypoints before any ambient path operation. True anchored variants require separate review.

## Public inspection/display behavior

```text
FileService::root_path() -> None for AnchoredRelative
FileService::capability_kind() -> explicit existing narrow kind (not FullAccount inference)
FileService::diagnostic_label() -> Option<&str> (crate-private diagnostics only)
```

Use only the separately named crate-private string-only diagnostic_label() accessor for this label. root_path()==None MUST NOT imply LocalDesktop or ApprovedFullAccount; callers must use the explicit capability kind and sealed native wrapper contract. Do not return it as &Path, concatenate it to authorize lookups, feed it back into a root constructor, or project it as an approved filesystem path. The narrow adapter may avoid exposing a generic FileService at all if Electra's final API chooses that wrapper.

## Sealed adapter requirements (Electra)

Bootstrap creates ONE trusted LocalDesktop RW issuer after actual Go acquisition and keeps it private. It already has account authority; the factory must NOT require an ApprovedRoot recreated by path rewalk. Reject peer ApprovedFullAccount and RO issuers. Any future selected ApprovedRoot support requires an actual separate scope proof, not a matching label; it is not part of this initial contract.

The factory receives that SPECIFIC original issuer plus an actual sealed prepared-root CreatedPrivate/VerifiedPrivate DIRECTORY result. It binds an anchored-relative native parent scope using the SAME actual child File/full identity. ChildIssuer then validates direct-child parent identity/leaf binding and derives further anchored-relative scopes. This is narrower derivation of existing authority, not arbitrary File→RW minting or another authority domain. No fresh approved_root(path), raw File/path factory input or pre-Go active issuer.

Validate original scope/binding, kind, actual RW provenance and current revocation/restriction before deriving. Constructor/adapter failure must leave the original sealed receipt in its native owner; a borrowed sealed input or an explicit retained failure is required.

A Verified ReadOnly preparation is not RW provenance. If needed, the original native RW issuer explicitly authorizes a descriptor-relative RW reopen; verify binding and compare its full identity to the original held result. Derive only from that NEW sealed RW result. No relabelling, raw-FD constructor, caller cleanup-authority bool or path regrant.

The configured label is diagnostics only. Scope comes from the existing LocalDesktop RW issuer and sealed original-root/direct-child binding, including its SAME creation/retention ledger where creation occurred. Do not describe the LocalDesktop issuer itself as ApprovedRoot-only confinement; only its returned child scope is narrowed.

## Native construction and fence (Tughril)

Construct the issuer ONCE after actual acquire/into_acquired and before owned control children. That is where the actual ownership.lost token and RootLifetimeGuard are available.

The restriction implementation captures clones of those SAME objects and synchronously checks loss at existing pre-stage and final commit checks under the existing mutation discipline. A watcher which revokes later is additional, not the sole fence. Derived caps retain the restriction/lifetime. No cross-process cleanup proof, authority recovery or cross-domain atomicity claim follows from the check.

## Metadata scope is not user-stage cleanup authority

Neither this Native AnchoredRelative metadata cap NOR a PathBound journal metadata ApprovedRoot authorizes ambient opening/deleting recorded absolute USER-stage parents. Full IDs, callbacks, cwd, diagnostic labels and persisted paths do not grant cleanup authority. Native journal/user-stage initialization, recovery and stage writes are gated; metadata-relative selectors must not become V1 absolute entries. Path-mode legacy compatibility is not security-certified by the native gate. Future Stage2 needs separately reviewed actual user-stage recovery authority/receipts and native-aware targets. Scoped prefs/approval metadata wrappers do not make native journals/uploads ready; no generic user-file workaround and default ingress remains off.

## Validation gates

- Existing path-root behavior remains unchanged.
- Native mode rejects every absolute/drive/UNC/prefix/traversal form on all OS.
- Diagnostic label is never opened and root_path never exposes it as authority.
- Same actual sealed directory/full identity is used; wrong kind/RO/revoked/scope mismatch fail closed.
- Explicit authorized RW relative reopen cannot adopt a different identity.
- Loss callback denies staging/commit even when watcher revocation is delayed; it cannot grant RO/revoked authority.
- Creation failure/panic retention remains in the original owner, including poison-safe conservative preservation.

Electra owns the single adapter/provenance API above; Walter owns the private mode/fence fields, constructor initialization and base dispatch; Tughril owns the one post-acquire native issuer/fence construction. Files lib/tests metadata and scoped strict all-target Clippy pass. Two owned resolution/restriction tests pass, but they are NOT acquired-token, sealed factory, ACL/ReFS/kernel or VFS acceptance. Native consumer activation still needs its own source handoff/review. No IPC, dependency or production Root/UI policy activation is proposed here.
