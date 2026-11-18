# Frontend physical-root proof proposal

Approved API contract, now implemented by Files and consumed by Desktop. Earlier lexical overlap checks and namespace tests did not prove physical separation on every filesystem. No new Host artifact is needed or authorized. The native seven-item allowlist stays unchanged.

## Exact proposed local API

Extend the existing `shellfox-files` capability implementation, not Desktop OS walkers:

```rust
pub struct DirectoryPlan; // opaque, retained native handles/identities; not a wire DTO

pub enum DirectoryRelation {
    Same,
    Ancestor,
    Descendant,
    Disjoint,
    Unproven,
}

impl FileService {
    pub fn plan_directory(self: &Arc<Self>, target: &Location) -> Result<DirectoryPlan>;
}
impl DirectoryPlan {
    pub fn configured_path(&self) -> &Path;
    pub fn revalidate(&self) -> Result<()>;
    pub fn relation_to(&self, other: &DirectoryPlan) -> Result<DirectoryRelation>;
    pub fn create_approved_root(&self) -> Result<Arc<FileService>>;
    pub fn create_private_approved_root(&self) -> Result<Arc<FileService>>;
}
```

Planning is read-only. Creation retains the original capability's access/revocation fence and cannot upgrade a read-only or selected-root capability. Both creation methods require write authority and return an approved-root capability, not LocalDesktop authority. They revalidate the exact retained plan, not reconstruct a new plan and silently adopt another directory.

`create_private_approved_root` delegates every missing component to the shared descriptor-relative private creation primitive and retains its actual created File/opaque HandleIdentity. Existing target privacy is verify-only, never repaired or certified as historically private at birth. Native trusted-parent/namespace custody requirements still apply; unsupported privacy/ACL proof fails closed. Ordinary user-folder creation remains separate. This adds no endpoint/prefs publication authority, Host creation by frontend or VFS-confinement claim.

Bound planning by the existing absolute-path byte limit plus an explicit component limit. Desktop needs exactly five plans, not an unbounded namespace crawler. No dependencies, native privilege, public/remote/mobile request or raw handle exposure is proposed.

## Required proof semantics

- Validate the original configured spelling before canonicalization. Refuse symlink/reparse redirects and parent traversal using the existing no-follow walk.
- Retain native identities for resolved components, not just path strings. Reuse the existing `content::same_identity` and `paths` implementation rather than a second OS walker.
- Existing roots compare native object identities and resolved ancestor identities. Alternate Windows spellings and case-insensitive macOS spellings must not appear disjoint merely because their strings differ.
- Missing roots retain the nearest resolved ancestor identity and unresolved suffix. Relationships must account for native namespace equivalence; Rust lowercasing or a canonicalized nearest-parent string is not sufficient.
- If native rules cannot prove a planned relationship, return `Unproven`. Desktop must fail closed before creation. Do not label an ambiguous missing-path plan Disjoint.
- Detect changed retained ancestors, existing roots and namespace replacements on revalidation. Never rebind to a replacement plain directory or follow a newly inserted redirect.
- Creation uses the held ancestor capability and no-follow descent. Recheck plan relationships at creation boundaries before returning/publishing roots or performing private-journal protection and file mutations. A newly generated alternate spelling must not let a later plan adopt an earlier root.

The exact native suffix-equivalence proof belongs to the local file-capability owner review. In particular, missing Windows names and generated alternate names need an explicit decision. Returning Unproven is preferable to a guessed guarantee.

## Desktop consumption after API review

1. `Options::parse` and `for_window` stay planning-only and create nothing.
2. Obtain the existing typed Host prestart result. Occupied handoff creates no frontend paths. Do not derive owner absence from directory planning.
3. Build/revalidate retained plans for Host, UI, device, run and editor from the original paths before any frontend mkdir.
4. Require Host/UI/device pairwise Disjoint. Require run/editor to be strict UI descendants, run/editor Disjoint, and neither to overlap Host or device.
5. Fixture/smoke still have a reserved Host-path plan but create only frontend paths. Missing Host paths are not permission to create a Host root.
6. Keep the validated topology alive through anchored creation, provider/backend construction and their lifetime guards. Existing DeviceColors and editor journal capabilities remain in use.

Affected Desktop files: `frontend_composition.rs`, `device_directory.rs`, `device_prefs.rs`, `editor_composition.rs`, their tests and composition documentation. No Host allowlist edit. Native proof implementation stays in narrowly reviewed `shellfox-files` path/service code.

## Required regressions

- Existing same-root and ancestor roots through alternate spellings resolve to Same/Ancestor, never Disjoint.
- Windows alternate directory spelling where the owned test filesystem supports it; Windows case variants; owned junction root/ancestor rejection.
- Case-insensitive macOS case aliases and case-sensitive volume behavior, with prerequisites reported explicitly.
- Missing siblings with shared resolved ancestors, suffix-equivalent plans, Host reserved/missing fixture paths and unproven plans. No frontend directory may be created on rejection.
- Ancestor/root replacement between planning, relationship validation and creation must not publish an adopted root.
- Actual default sibling `.desktop`/`.device` plus UI run/editor composition remains valid where physically proved; no Host-root additions.

The full platform matrix remains pending. Current Windows Desktop consumer regressions cover cold default/explicit composition, case aliases, ambiguous missing names, revocation and unrecorded creation; they do not establish macOS/Linux behavior or whole-app acceptance. Earlier 300-test and strict-Clippy checkpoints are historical. Creation-time ACL privacy and producer leaf-security review remain separate gates. No GUI, network or native privilege trial is authorized by this proposal.
