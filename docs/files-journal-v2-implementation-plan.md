# Journal V2 implementation planning and producer handoff

Status: detailed planning only, following `files-journal-v2-schema-proposal.md`. Electra owns the active journal/private publication modules. No Stage2 reader/writer/recovery edit or activation is authorized by this document. Content SHA and application wire contracts remain unchanged.

## 0. Metadata custody is NOT user-stage recovery authority

This separation applies to BOTH PathBound and AnchoredRelative metadata roots. The original metadata issuer authorizes bounded metadata read/publication in its own held root. It does NOT authorize ambient opening/deleting USER-stage parents from persisted absolute paths. Identity evidence is necessary, never permission. Callbacks only restrict existing authority; they create no cleanup grant.

Remove the internal LocalDesktop recovery mint from the future design. Stage2 must receive a separately accepted actual user-stage recovery authority/receipt, retaining its original scope, access/revocation and ownership lifecycle. Its exact owner/provenance/resolution API is an OPEN review gate, not a newly approved constructor. It must resolve/check a stage through that authority, not call `paths::open_absolute(record.parent)` because a metadata cap or ID was available.

A native-aware user-stage target contract is separately required; metadata-relative selectors cannot be persisted as V1 absolute user-stage paths, and a diagnostic label/cwd cannot convert them. Immediate native journal/user-stage gates remain ON. Existing path-mode legacy compatibility is NOT security-certified by native gating, full IDs or these schema plans. No active journal edits while Electra owns those files.

## 1. Original journal owner and bounded slots

Proposed internal owner: `JournalPreparationOwner`, acquired from the ORIGINAL anchored journal issuer before native preparation. It retains the original root/authority, actual lease when acquired, both bounded read results and one publication/recovery slot. Keep it outside `catch_unwind`; neither a returned error nor mutex poison may dispose of its only custody.

Bounds:

- Main and next: at most4 MiB EACH, combined at most8 MiB. No historical document queue.
- At most32 entries per closed document. While both documents are unresolved they remain two distinct documents, not a merged enlarged active journal.
- Exactly ONE native publication/recovery operation slot per journal owner. Reserve its memory/custody capacity before native preparation.
- One bounded recovery disposition per original entry, with no timeout/TTL eviction.
- No new transfers while journal admission/recovery is blocked. Do not advertise free capacity by dropping unresolved entries.

The slot is registered BEFORE calling `prepare_private_publication_at`: original parent/full identity, stage/destination leaves, expected current-main version and possible-artifact state. On success store the real `OwnedPrivatePublication` before serialization or other fallible work. On `PrivatePreparationFailure`, consume its real `into_parts()` and store any retained attempt in that SAME slot. If the caller catches a panic before a result/child FD is stored, preserve the pre-registered possible-artifact row; never infer NoCreate/NoWrite.

A keeper may inspect/verify and explicitly preserve stale evidence after lock poison, without clearing poison or restoring normal write authority. The failure result must retain this owner; no generic FileError/string/anyhow conversion which disposes of its custody. Cleanup failure stays in the owner. Explicit preservation is not unlink, NoOwner or durable journal success.

## 2. Stage identity before serialization — additive producer request

Actual producer already published:

- `prepare_private_publication_at(...) -> Result<OwnedPrivatePublication, PrivatePreparationFailure>`
- owned one-shot bounded `write_all`, retained publication/cleanup phases
- `PublishedPrivate` issued only by successful publication of the real CREATED object

Missing narrow API for agreement with Electra:

```rust
// PROPOSED additive getter on the REAL retained attempt:
OwnedPrivatePublication::preparing_identity(&self)
    -> Result<&HandleIdentity>;
```

It succeeds only for the actual retained created stage in Preparing state. It exposes the existing opaque equality identity, not `&File`, a writer, raw handle, stage constructor or a synthetic PrivateVersion. The journal converts that actual identity through the separately approved crate-private V2 evidence codec, then serializes documentIdentity while the attempt remains in the original slot.

No public serde/raw-field constructor is added to HandleIdentity. The codec has exact closed Windows64+128/Unix dev+ino encoding and compares decoded evidence to the SAME provider; it does not create filesystem authority.

Write and publish then use only the retained attempt's reviewed methods. A serialization/write/readback/publication failure does not consume the slot or reset it to Empty. Partial bytes are not the old empty-file version. Stage identity availability must not be implemented by leaking an unrestricted writer handle.

## 3. PrivateRead is observation, NOT a publication/recovery receipt

`PrivateRead`/`PrivateVersion` establishes bounded current bytes, current identity/version and current-private verification. Parsing V2 adds structural validation. NONE of these alone is:

- original exclusive creation;
- validated main/next lineage under the actual lease;
- authorization to rename/delete;
- `PublishedPrivate` or permission to manufacture it.

No `From<PrivateRead>`/`From<PrivateVersion>` to PublishedPrivate, CreatedPrivate or a recovery receipt is allowed. An identity/version plus a boolean `validated` argument is not a replacement for a sealed proof.

## 4. Qualified-next recovery: DISTINCT receipt and owner

Proposed internal type, separate from all creation receipts:

```rust
pub(crate) struct ValidatedJournalNext; // private fields/constructor, no serde
pub(crate) struct OwnedJournalNextRecovery; // retained existing-stage operation
pub(crate) struct RecoveredJournalDocument; // NOT PublishedPrivate
```

Only the journal lineage validator may construct ValidatedJournalNext, after all of these actual checks:

1. Original root/authority/namespace still current and actual exclusive journal lease held.
2. Both main and next were bounded-read and then revalidated under that lease; exact closed V2 profiles, no V1/unknown document.
3. Root identity and both document self-identities match the retained actual handles; native identity/privacy checks succeed.
4. next.base matches the actual current main full identity+complete content SHA, or proven absence. Every parsed bound/path/state/kind is valid.
5. Retain the original root and existing next handle, exact current-main condition, parsed next bytes/version and current logical RW/scope/revocation fence in the receipt/owner.

The native producer must consume/borrow that CONCRETE validated receipt into a distinct OwnedJournalNextRecovery, not accept caller-supplied flags. It may internally acquire narrowly required publication access relative to the SAME retained parent and recheck exact next identity/version; no absolute reopen or unrestricted writer escape. A read-only open cannot silently be treated as a created/publishable stage.

The recovery owner does NOT rewrite next bytes. It has retained prepare/rename/verify/durability/uncertain/preservation states and keeps original custody on every failure. A successful verified recovery yields RecoveredJournalDocument for that actual existing object, not PublishedPrivate. No public conversion between those receipt domains.

Exact native recovery API/signatures and ownership must be agreed with Electra BEFORE implementation. Until that producer exists and is validated, even structurally qualified V2 next remains retained/pending with no rename/deletion. Initial Stage2 may safely stop there rather than synthesize an unavailable receipt.

## 5. Admission and refusal ordering

Approved policy: ALL V1 automatic destructive recovery/rewrite is refused initially. Unknown/future/malformed profiles are preserved. No guessed filesystem origin or padding/widening V1 evidence.

Order before cleanup:

1. Retain/verify the original root and inspect BOTH main and next read-only, bounded and strict. No clear_next, repair, stage recovery, overwrite or default journal creation.
2. Refuse legacy/unknown/invalid profiles, retaining original bytes/namespace. Surface MigrationRequired/Unsupported, not Ready(empty).
3. Existing V2 requires its existing actual exclusive lease, verify-only. Missing lease is blocked, not a repair opportunity. Reread/revalidate both documents after acquisition.
4. For genuinely absent main+next, exclusively prepare new V2 under the native namespace/lease rules and recheck absence before committing. An unexpected document stops mutation and never authorizes unlinking a lease name.
5. Validate lineage. A qualified next needs the DISTINCT recovery receipt above; an unqualified next preserves BOTH documents and blocks. Never blindly clear it.
6. Only a selected, actually authoritative V2 main proceeds to entry recovery. No provisional read/version promotes itself to published state.

## 6. Intent+existing-file and other unresolved entries

Keep the original serialized EntryV2 unchanged while unresolved. Proposed bounded in-memory disposition is separate from the schema:

```text
Unexamined
AbsentObserved              // no deletion, NOT durable forget yet
DeletedExact                // actual permitted cleanup, NOT durable forget yet
Unresolved(reason)
ForgetPublicationPending    // still occupies its original entry slot
```

Closed diagnostic reasons include MissingRecoveryAuthority, MissingOriginalFileIdentity, ParentMismatch, FileMismatch, PrivacyUnavailable, NativeIdentityUnsupported, CleanupUncertain and PublicationPending. Surface counts/reasons through the retained native owner; do not log paths/bytes or invent a successful deletion.

Rules:

- No separately accepted user-stage recovery authority: MissingRecoveryAuthority. Do not open ambient recorded parents merely to discover/check a matching ID. Metadata ownership, kind, callback, cwd or label cannot fill this gap.
- Intent + existing file: MissingOriginalFileIdentity. Retain entry and file untouched; name, zero length, today's identity or current private ACL does not identify the historical original.
- If the ORIGINAL live creator/stage owner still holds the actual original returned File/evidence, a separately validated same-process completion may persist identification. A restart/path reopen cannot substitute for that owner.
- Even an identified entry with matching full IDs stays blocked without accepted user-stage authority. Parent mismatch/unsupported provider or uncertain cleanup: preserve original entry/evidence and block. No TTL/GC, retry under fresh authority or slot reuse.
- Observed absence or exact successful deletion only prepares removal in a candidate next document. The original entry continues to consume quota until that exact updated document has an actual verified publication/recovery receipt and the required durability outcome.
- Publication failure/uncertainty/durability-unconfirmed: retain the old authoritative entry and pending attempt. Do not mark durable forget, free the slot, replay publication or rewrite empty V2.
- With unresolved/pending entries, initial policy is blocked new transfers, even if fewer than32 records exist. This is bounded and visible rather than silently dropping debris.
- Explicit stale preservation keeps parent/evidence and records disposition; it does not report journal recovery success or erase persisted rows.

## 7. Planning ACK required before Stage2 edits

Ask Electra to confirm:

- Preparing-only opaque stage-identity getter on the real owned attempt.
- Journal slot preparation/retained failure and keeper integration, including poison handling.
- Concrete ValidatedJournalNext handoff and distinct retained existing-metadata-stage recovery/result types, or an explicit initial pending-only policy. Neither metadata transaction receipt grants user-stage recovery authority.
- Separately accepted actual user-stage recovery authority/receipt and native-aware target contract before any recorded-parent open/delete. No internal LocalDesktop mint or persisted-path authority.
- Exact conditional publication/durability contract used to promote authoritative main and commit durable forget.

Then agree the private evidence codec/schema ownership and exact active journal file edits. Required tests remain bounded/mechanical refusal and crash-boundary cases plus separately permitted native publication/ACL/lock/cleanup fixtures. No ReFS/VFS/global privileged mutation acceptance follows from this planning.
