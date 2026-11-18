# Files CAS and journal identity migration proposal

Status: Stage1 in-memory Snapshot/CAS full-identity migration is implemented and leaf-checked; persisted journal migration remains planning only. No revision/wire contract or V1 evidence upgrade is implemented by this document. ReFS recovery remains unaccepted. No exploit reproduction or native ReFS proof is claimed.

## Shared provider

Use the ONE opaque `HandleIdentity` implementation owned by Walter. Windows identity is the full `FILE_ID_INFO` volume64 and ID128 from the actual held handle; Unix identity is native device/inode. Unsupported capture prevents a comparison from proving equality. Do not fall back to cap metadata, a filesystem label, path text or a guessed high half.

The pinned cap provider stores a u32 Windows volume serial and u64 file index. Both are narrower than the required native identity. SHA-256 revision, length and times are additional predicates, not substitutes for identity.

## Bounded in-memory CAS batch

Proposed ownership must be agreed before implementation:

1. Add full handle identity to `content::Snapshot` when its actual File is acquired. `same_version` compares that identity plus its current attributes; keep all existing revision, size, mtime and native change-time checks.
2. Replace held-object comparisons in stage validation, replacement preparation and owned cleanup with this provider. Metadata preceding an open remains type/reparse rejection only. A namespace binding must compare actual no-follow opened handles, never manufacture an identity from Metadata.
3. Upload retention entries currently retain Metadata. Capture a full identity at initial file acquisition instead. Keep the same ownership, retention and no-recursive-cleanup policies.
4. Keep current conditional publication and no-automatic-retry behavior. Do not call check-then-rename an OS compare-exchange against an unrelated external writer. The final external-writer interval remains a declared limitation.
5. Do not change generic IPC sizes, SHA-256 revision syntax, terminal generation/control semantics or user-file permission restoration in this batch.

Stage1 now captures actual full identities in Snapshot/held-file/parent/transfer/upload in-memory custody. That does NOT migrate or bless persisted evidence or recovery authority.

## V1 evidence is not V2 evidence

Existing version1 records contain only `{dev, ino}` numbers for parent and file. Never fill missing high bits using a current object, reinterpret the record as full identity, or silently deserialize it into the opaque live provider type.

Approved initial policy: refuse ALL automatic destructive V1 recovery/rewrite before cleanup, preserving both main/next and user stages. Unknown/future versions and unsupported providers are preserved too. A filesystem name cannot repair V1 evidence; no default empty-V2 rewrite.

Preferred conservative Windows rule: decline automatic deletion from projected V1 records unless a separately reviewed provider/custody argument proves *both* volume and file uniqueness. Merely saying NTFS is not enough to repair the volume32-versus-volume64 projection. Keep the V1 records and matching stage names untouched and report the unresolved cleanup, rather than scanning projects or trying candidate paths.

Unix V1 evidence also needs provenance/precision review. Legacy Node JSON used numbers, so a serialized integer is not proof that an original large native integer was represented exactly. Restrict any approved legacy recovery to an explicitly reviewed exact subset; do not infer missing precision from the current inode.

## Recovery authority gap in BOTH modes

A metadata ApprovedRoot in PathBound OR AnchoredRelative mode is NOT permission to ambient-open/delete absolute user-stage parents. Identity/private-state evidence is necessary, never authority. Callbacks, diagnostic labels, cwd and persisted paths cannot grant cleanup rights; no internal LocalDesktop recovery mint.

Future Stage2 requires separately accepted actual user-stage recovery authority/receipts and a native-aware target contract. Missing authority blocks recovery even if every full identity matches. Immediate native gates do not security-certify existing path-mode compatibility. This remains planning-only; do not edit Electra's active journal implementation concurrently.

## Explicit version2 journal

Proposed version2 should use a distinct schema and filename. A possible private codec contains tagged Unix device/inode or Windows volume64/ID128 encoded as exact fixed-width strings. Windows file-ID bytes retain the API byte order, not a guessed GUID conversion. Live `HandleIdentity` fields/constructors stay private and are not given public serde implementations.

An additive provider-owned evidence encoder/comparator may be needed. Journal code can compare validated persisted evidence to a newly captured full identity without constructing a live authority/identity from untrusted JSON. Agree that API with Walter before coding it.

- Capture parent identity from the original held parent, not a reopened label.
- Persist write-ahead intent before exclusive private-at-birth creation; persist actual created identity before bytes.
- Recover only explicitly recorded names, exact held/native identities and ownership. No project-directory scans or deletion of aliases/replacements.
- Keep the same original root/lease/custody for V1 inspection and V2 writing. No independent cleanup verifier or scope.
- Bound combined old/new records and bytes. Unsupported/quarantined records must not cause unbounded disk reservations or a silent ready state.
- Define crash ordering and publication before accepting a dual-format transition. A V1 unsupported record remains V1 and is never rewritten as newly proven V2 evidence.

## Tests and acceptance

Required mechanical tests distinguish equal low64/different high64 file IDs and equal low32/different high32 volume IDs. Unsupported queries propagate as unsupported/unproven. These tests do not prove native ReFS behavior.

Add staged tests for source replacement with equal content, parent replacement, wrong/reused cleanup names, identity capture failure, retained V1 unsupported records, JSON precision boundaries, interrupted V2 intent/identify/publish, and lease exclusion across version readers. All cleanup stays bound to original handles and evidence. Native filesystem/platform acceptance requires its own evidence; no SDK/window/network execution is requested by this proposal.
