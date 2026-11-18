# Files native identity correction — proposed scope

Status: first bounded batch approved and metadata-checked. Actual shared provider is Electra's existing HandleIdentity implementation; no second provider was created. No race/exploit reproduction or ReFS/platform acceptance claim.

## Provider evidence

`content::same_identity` compares only cap metadata `dev()/ino()` (u64). The installed cap-fs-ext 3.4.6 `src/metadata_ext.rs:19` explicitly says `FIXME: On Windows' ReFS, file identifiers are 128-bit`. Its Windows implementation at lines 94–103 maps dev to the u32 volume serial and ino to `_WindowsByHandle::file_index` (u64). cap-primitives 3.4.6 `src/windows/fs/metadata_ext.rs` likewise stores `file_index: Option<u64>`. There is no full-width identity guarantee here.

Consequently the current DirectoryPlan retained ancestry, relation checks, created-prefix journal and Windows ReFS namespace admission cannot establish full native identity. Merely dropping ReFS from `directory_names` is insufficient: already-existing directory relations and capability root revalidation still use the lossy comparison, and the volume projection is also narrower than FILE_ID_INFO.

## Proposed one private provider

```rust
pub struct HandleIdentity { /* private native fields; opaque Clone + Eq */ }

HandleIdentity::from_file(&std::fs::File) -> Result<HandleIdentity>;
HandleIdentity::matches(&self, &std::fs::File) -> Result<bool>;
// Additive crate-private cap Dir borrowing adapter, same provider:
HandleIdentity::from_directory(&cap_std::fs::Dir) -> Result<HandleIdentity>;
```

Use the original live native handle only. Windows queries `GetFileInformationByHandleEx(FileIdInfo)` and retains all 128 identifier bits plus the 64-bit volume value, as transport already does. If that native query is unsupported, return Unsupported; no legacy file-index fallback, path inference or successful comparison of unavailable identities. Unix uses native full dev/inode metadata. The returned public equality type has private fields, no raw-field constructor and no serde. The cap Dir adapter duplicates the same original native handle, never reopens its path. None of these identities confer authority. No new dependencies or wire fields.

Electra's private-birth primitive must use this same provider and return the actual created handle plus this identity, not an independent 64-bit token. Its journal anchor must compare the actual held directory/security handles with the provider. Do not edit her journal module/declaration concurrently.

## First bounded implementation batch

1. Capture native identity for every retained directory handle, including the original anchor. DirectoryWalk stores those identities separately from metadata used for type/link checks.
2. Replace DirectoryPlan ancestry, relation, revalidation and created-prefix ledger identity comparisons with full identities. Unsupported identity capture prevents a plan/proof/creation; do not convert it to Disjoint.
3. A pre-open `symlink_metadata` result cannot supply full Windows ID. Keep it for no-follow/type rejection only. Prove edge stability using two descriptor-relative no-follow opens and compare their full handle identities while retaining the originals, rather than comparing a path metadata projection to a held handle.
4. Reuse the same provider in capability root revalidation and private creation/anchor checks. Preserve all original capability scope, write/revocation checks and no-follow/collision rules.
5. Mechanical tests must distinguish IDs with equal low64 and different high64 bits and propagate Unsupported. These are not a ReFS/native proof.

## Existing CAS assessment

The same projection is not confined to DirectoryPlan. `content::Snapshot/same_version` uses it for read/edit/transfer version checks. Explicit identity guards also occur in `staged_write`, `mutations`, `platform`, `upload_private/upload_storage`, and Electra's `journal_anchor/journal_storage`. Hash/revision, modified/change time and length checks are additional predicates, not a replacement for identity uniqueness.

Proposed follow-on under the same provider, not a rewrite of CAS semantics:

- Add full identity to `content::Snapshot` from its actual File. Compare that identity in `same_version`.
- Replace held-file/held-directory comparisons with native identity queries. Where only cached Metadata is currently retained (for example upload entries), retain the original full identity at initial handle acquisition instead.
- Keep path metadata only for no-follow/type checks and diagnostic attributes. Never declare native identity from it.
- Retain existing conditional publication, revision/hash checks, original-handle cleanup, no-follow and no arbitrary replacement guarantees. No mutex/transport protocol redesign.

DirectoryWalk/Plan, capability root revalidation and the created-prefix ledger now use opaque full handle identities. Pre-open metadata is type/reparse checking only; stable edges compare two relative no-follow held handles. Identity-query failure returns Unsupported before proof/mutation. No projected ReFS fallback remains in those paths.

CAS/persisted evidence is NOT covered by that checkpoint. See `docs/files-cas-identity-migration-proposal.md` for its separate staged review, including strict old-V1 refusal before recovery mutations. Existing ReFS CAS/recovery remains unaccepted; no native ReFS pass is claimed.
