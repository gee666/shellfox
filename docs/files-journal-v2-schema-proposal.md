# Exact private journal V2 schema and recovery ordering — proposal

Status: schema/refusal direction approved for detailed implementation planning ONLY. No active journal edits are approved here. No persisted V1 schema, journal reader/writer or recovery mutation is changed by Stage1. See `files-journal-v2-implementation-plan.md` for bounded unresolved entries and the distinct validated-next recovery/producer handoff. V1 ReFS evidence remains unaccepted; no publication/VFS activation or historical proof from today's object.

## Recovery authority is separate in BOTH modes

A journal metadata ApprovedRoot—PathBound OR AnchoredRelative—authorizes its own held metadata namespace, not ambient resolution/deletion of recorded absolute USER-stage parents. Full IDs, current privacy, callbacks, diagnostic labels, cwd and persisted paths are NOT permission. StagingJournal must not internally mint LocalDesktop authority for recovery.

Stage2 requires a separately reviewed actual user-stage recovery authority/receipt and target contract. Resolve/check a user-stage parent only through that accepted authority, never `paths::open_absolute(record.parent)` justified by a metadata root. Native user-stage initialization/writes/recovery are immediately gated. Existing path-mode compatibility is not automatically security-certified. This remains planning-only until the exact owner/authority handoff is reviewed; do not edit Electra's active journal code under this proposal.

## Closed JSON format

Every object below rejects unknown and duplicate fields. No optional identity fields and no unknown state variants. Existing limits remain: document ≤4 MiB, ≤32 entries, paths pass existing absolute/no-device/no-ADS/no-traversal validation (≤32767 UTF-8 bytes), canonical recognized stage leaf, stage.parent exactly equals parent, no duplicate stage keys.

```text
DocumentV2 = {
  "version": 2,
  "rootIdentity": IdentityV2,
  "documentIdentity": IdentityV2,
  "base": BaseV2,
  "entries": [EntryV2, ...]
}
BaseV2 = {"kind":"absent"}
       | {"kind":"present","identity":IdentityV2,"revision":Sha256}
EntryV2 = {
  "stage": AbsolutePath,
  "parent": AbsolutePath,
  "parentIdentity": IdentityV2,
  "state": StageV2
}
StageV2 = {"kind":"intent"}
        | {"kind":"identified","fileIdentity":IdentityV2}
IdentityV2 = {"kind":"windows-file-id128","volume":Hex16,"fileId":Hex32}
           | {"kind":"unix-dev-ino","device":Hex16,"inode":Hex16}
```

Hex16/Hex32 are exactly16/32 lowercase hexadecimal characters, no prefix, whitespace or JSON-number alternative. The Windows fileId encodes the native16-byte Identifier array in order; volume and Unix values encode the exact native u64 value. Sha256 is the existing64-character lowercase SHA-256 string of the complete previous document bytes. Native unavailable/unproven ID rejection remains the provider's rule. Identity kinds must match the current native platform and each captured object; no cross-kind equality or padding V1 ino into V2 fileId.

`rootIdentity` is captured from the original held journal-root handle. `documentIdentity` is captured from the actual exclusive-created metadata stage File BEFORE its bytes are serialized/written. Rename to the main name preserves that exact identity. `base` records the original current-main full identity/content revision, or proven absence, under the existing conditional-publication boundary. It never grants stage-parent filesystem authority.

This is PRIVATE evidence serialization, not serde on public HandleIdentity. Keep its public fields/constructor opaque. Add only a closed crate-private persistence adapter which captures actual provider identity from held handles and compares decoded evidence against newly retained actual handles. Never deserialize arbitrary JSON into a public native-handle identity/authority object.

## Writer preparation and publication

V2 changes private preparation explicitly, not silently under the V1 label:

1. Hold the current host/journal namespace custody and actual acquired exclusive journal lease.
2. Validate original RW/scope/revocation, root binding/privacy, entry bounds and expected current-main Snapshot/revision.
3. Exclusively create `.file-transfer-stages.next` through the compiled private-birth primitive. Do not inherit then repair or adopt a collision.
4. Capture that actual created File's full documentIdentity; serialize the closed V2 document including exact base. Enforce byte bound BEFORE writing.
5. Write/sync privately, recheck original root/caller authority/current-main condition and publish using the separately reviewed anchored actual-file primitive. Keep its actual cleanup custody on any failure.
6. Verify published main full identity and bytes; update in-memory expected Snapshot. A failure is not an empty successful journal.

The metadata stage is created before final serialization because its actual file identity is part of the document. This prep-order change requires explicit approval. Existing user-content SHA revisions and user-file conditional/publication semantics do not change.

## Refusal BEFORE clear_next/recover/persist

Current V1 startup calls clear_next before parsing the main document. V2 must not preserve that ordering.

For an existing journal root:

1. Retain/verify the original root and perform bounded READ-ONLY inspection of BOTH main and reserved next through actual no-follow handles. Do not clear, rename, delete, repair permissions, recover a user stage or persist during this inspection.
2. V1 in either file: return migration-required and preserve both files/user stages. Unknown version, malformed closed document or unsupported identity: fail closed likewise. Do not accept by shape or discard unknown fields.
3. Existing V2 state requires the existing named exclusive lease, acquired verify-only. Missing/invalid lease is not automatically repaired for an existing document. After lock acquisition, reread/revalidate the exact root and both document snapshots/profile decisions before any mutation.
4. For a genuinely absent main/next, new-journal setup may exclusively create/acquire its private lease, then recheck absence/current root before V2 creation. If an unexpected document appears, stop without recovery/publication; never unlink a lease name.
5. Validate rootIdentity and documentIdentity against the actual held root/document Files. Validate next.base against the actual current main full identity+SHA, or actual absence.
6. Only an exact qualified V2 next transaction can proceed to its reviewed conditional publication. A lineage mismatch preserves BOTH files and returns conflict. No unconditional clear_next remains. Completing a qualified next requires a DISTINCT validated lineage/recovery receipt and separately approved anchored publication; otherwise retain it and report pending. PrivateRead/version alone never manufactures PublishedPrivate.
7. Only then, AND only with the separately accepted actual USER-stage recovery authority/receipts, recover V2 entries under full identity, private ownership, namespace and caller checks. Metadata-root ownership by itself never permits ambient opening/deleting the recorded parents. Persist removal of an entry only after an actual allowed result, not an assumed delete.

The initial read is not treated as a lock/commit proof; it is repeated after the actual lease. Legacy refusal must precede all destructive cleanup/overwrite hooks. No absolute-path publication/removal fallback is authorized.

## Entry crash windows

- Before intent persistence: no automatic recovery authority exists.
- Durable intent but no stage: after exact parent identity and no-follow NotFound, the entry may be resolved without deleting anything, under current exclusive namespace custody.
- Stage created but identify not durably published: intent has NO file identity. If a file exists, preserve it and the intent as unresolved. Length zero, recognized filename and today's file identity do not prove the historical original.
- A qualified durable next may carry the actual original identified evidence. It must be completed/retained using the lineage rule, not thrown away by clear_next and not invented from today's object.
- Identified entry: delete only through separately accepted actual user-stage recovery authority, after exact recognized original-object/full parent/file evidence and current privacy/capability checks. Missing authority remains blocked even when every ID matches. Mismatch/Unsupported preserves custody/evidence and fails closed.
- Publication complete before forget: the stage name may be absent. Resolve by exact parent identity plus actual no-follow absence; do not inspect/delete the published destination by guessing it from a stage name.
- Cleanup succeeds but updated journal persist fails: keep original evidence; a retry must handle actual absence safely. Never claim durable forget merely because unlink succeeded.

These guarantees rely on the reviewed trusted namespace/ownership model, not hostile same-account or global Unix VFS confinement. Full IDs are necessary evidence, not new OS privilege.

## Approved initial legacy policy

Refuse ALL automatic destructive V1 recovery/rewrite initially, especially Windows V1 which lacks platform/filesystem/width provenance. Mandatory minimum is unconditional V1 ReFS refusal; do not infer NTFS origin from today's path or upgrade projected IDs by querying current objects.

Leave the original V1 bytes/stages unchanged and surface migration-required. Do not rewrite it as empty V2, drop entries, promote V1 next, pad64-bit IDs, infer missing identity from empty content, or release an old record's authority because startup timed out.

Any later legacy operator tool must use explicit current native/user cleanup authority, not manufacture original-file proof. It may preserve bounded legacy bytes for audit, but must not pretend those bytes became V2 evidence. No automatic V1 converter is proposed here.

## Separate validation/ownership gates

Electra owns active private/journal implementation; Walter supplies the same compiled full identity provider. Agree exact edits before changing those files. Stage1 Snapshot improvements do NOT upgrade V1.

Required mechanical tests: strict width/kind/duplicate-field rejection, exact bit round-trip, V1/unknown refusal before cleanup hooks, both-main/next ordering, base mismatch preservation, intent-with-existing-file refusal, qualified-next crash windows, Unsupported propagation, unchanged user SHA/CAS behavior. Native publication/privacy/lock/cleanup and actual ReFS recovery remain separate acceptance fixtures; no pass is implied by metadata or mechanical tests.
