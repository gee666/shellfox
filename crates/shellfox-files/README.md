# Shellfox files

This crate ports the active local file service, chunk transfers, preview assets, image saves and private terminal upload inbox. It has no Electron, GPUI or network dependency. Existing Node/Electron code is unchanged.

## Authority

Only trusted application code constructs capabilities.

- `FileService::approved_root(absolute_directory, Access::ReadOnly | Access::ReadWrite)` pins a user-selected directory. Requests accept relative paths or component-contained absolute paths. A sibling sharing a string prefix is not inside the root. Parent traversal is rejected rather than normalized.
- `FileService::local_desktop(access)` grants local desktop access to absolute account paths under OS permissions. This is not a directory sandbox and must never be constructed from a remote or mobile request.
- `capability_kind()` reports the distinction. `root_path()` returns the selected root, if any.
- `revoke()` waits for a mutation already committing. After it returns, new operations fail. Reads recheck authorization before returning content.

The host owns actor authentication, approval, non-elevated profile selection and terminal control. Transfer and upload callbacks must consult current policy, not a captured approval boolean. The caller holds its approval/control commit guard through finish. The library checks again after stream inspection, permission copying and flush, immediately before publication. It does not start a listener or initiate an upload automatically.

## Local file API

Operations return `Result<T, FileError>`. Errors have a typed code and a short message, without native error strings or automatic retries of uncertain mutations. Request structs reject unknown serde fields. Methods also validate paths, revisions and byte limits at runtime.

| Method | Request | Result |
| --- | --- | --- |
| `list` | `Location { path }` | `FileList { path, parent_path, entries, truncated }` |
| `stat` | `Location` | `FileStat { path, kind, size, readonly }` |
| `read_text` | `Location` | `TextFile { path, text, size, revision }` |
| `read_file` | `Location` | `ReadResult::Text`, `Image` or `Binary`, with actual MIME, size and revision |
| `preview` | `Location` | Existing `Preview` API retained |
| `write_text` | `WriteText { path, text, revision }` | `WriteResult` |
| `create_file` | `CreateFile { path, text }` | `WriteResult` |
| `write_image` | `ImageWrite { path, revision, mime_type, bytes }` | `WriteResult` |
| `read_asset` | `AssetRequest { path, asset_path }` | `FileAsset { mime_type, bytes, size }` |
| `create_directory` | `Location` | `()` |
| `resolve_directory` | `Location` | `DirectoryResolved { cwd }` |
| `complete_directory` | `Location` | `DirectoryCompletions { matches }` |
| `rename` | `Rename { path, destination }` | `()` |
| `delete` | `Delete { path, kind }` | `()` |

Inline text is limited to 128 KiB and preview/asset reads to 2 MiB. NUL and invalid UTF-8 text are rejected; BOM and CRLF are preserved. Image classification follows existing PNG/JPEG/GIF/WebP/ICO/AVIF magic checks. It does not decode complete images. Inline image saving is limited to 512 KiB and existing PNG/JPEG/WebP with matching MIME, magic, extension and revision. Binary reads do not return file bytes. Adapters encode bounded byte results as the protocol's canonical base64/data URL; this crate does not fetch URLs.

Assets require an existing HTML/XHTML or Markdown-family base file and stay beneath that file's parent directory. Queries, fragments, absolute paths, traversal and link escapes are rejected. The allowlist is CSS, JS/MJS, JSON, SVG, supported raster images and WOFF/WOFF2/TTF/OTF fonts. Renderers still need their existing preview isolation and decoder limits. Returned SVG/HTML text is not trusted application code.

Listings stop at 500 entries or a conservative 256 KiB serialized-response budget. Completions retain at most 100 sorted candidates and return at most 64 KiB. Neither needs an unbounded directory allocation. Symlinks and Windows reparse entries may be listed or statted, but not traversed, read, written, renamed or deleted. Directory creation is recursive and idempotent. Delete requires a regular-file or empty-directory kind and never recurses. Rename is same-filesystem and atomically refuses an existing destination, including an empty directory.

`FileContext { cwd, home }` is trusted host context, not a frontend cwd hint. `resolve_location` expands relative/home paths only within the existing capability. Directory prompt methods also support enclosing quotes through `resolve_directory_with_context`, `create_directory_with_context` and `complete_directory_with_context`. No method guesses a running terminal's live cwd or relabels a launch cwd as live. Without context, approved-root relative paths stay root-relative; account home is not silently equated with the approved root.

## Chunk transfer API

Create `TransferRegistry::new(TransferLimits)` and call `initialize_journal(private_host_userdata_directory)` at host startup. Begin operations are disabled until initialization succeeds. Never derive the journal directory from a remote request. One registry serves the host; adapters serialize mutable access.

The exported methods are `begin_read`, `begin_write`, `read_chunk`, `write_chunk`, `finish_write`, `cancel`, `cancel_actor`, `sweep`, `dispose` and `usage`. Begin accepts `Arc<FileService>`. Every transfer retains that exact capability, owner and connection generation; later chunks cannot select another root. `TransferActor::new(owner, generation)` is trusted and is not deserializable request authority. `TransferAuthorization::check(actor, capability, write)` must consult live policy. A host-wide sweep provider must understand every stored actor, not just one request's connection.

`BeginRead` contains `path` and optional `asset_path`. `BeginWrite` contains `path`, `size`, `mode`, optional `revision`, `content` and optional `mime_type`. Replace mode requires a revision; image content requires an editable MIME. Read/write chunk structs carry a UUID transfer ID and exact byte offset; writes carry bounded `Vec<u8>`. Responses report the next offset, EOF, revision and detected content/MIME. The transport owns canonical base64 validation and frame bounds before decoding.

Limits match the active service: 192 KiB chunks, default 256 MiB per file, maximum 1 GiB, 8 MiB editor text, 64 MiB edited images, two active transfers per owner, eight total, and staged reservations totaling at most twice the configured file limit. Idle TTL is at most five minutes. Streaming SHA-256 and UTF-8 validation use bounded buffers, including code points split across chunks. Source identity/version is checked before and after reads. Before publication, failed checks can dispose only the original live stage. Once publication may have occurred, errors preserve the outcome and never trigger replay or guessed cleanup.

Exact last-chunk retries are idempotent. Finish retries return the previous outcome without publishing again, with at most 128 receipts retained for the idle TTL. Begin has no idempotency key and must not be retried automatically after an uncertain transport result. Owners may discard their own partial after losing approval. The host must call actor cancellation on disconnect/control loss and sweep expired operations.

## In-process desktop text editor

`EditorBackend::new_anchored(capability, journal)` initializes one app-owned backend from the EXACT `Arc<FileService>` approved RW journal root returned by the directory producer. `TransferRegistry::initialize_journal_anchored(journal)` provides the same retained-root initialization for other callers. Journal authority must be PathBound `ApprovedRoot`, originally writable and not revoked. AnchoredRelative native metadata capabilities are rejected before directory protection, lease acquisition, `.next` cleanup, V1 recovery or journal publication. Editors also reject native user-file capabilities before journal initialization. AnchoredRelative user stages remain disabled. PathBound editor/transfers use V2 and separate live user-stage custody; the private upload inbox uses that same V2 registry. Local-desktop/full-account capabilities and read-only roots are rejected as journals before ACL or file mutation. The original Arc, descriptor and inherited revocation chain remain authoritative.

The application supplies a stable, validated frontend namespace, separate from Host and device storage. Do not derive it from a per-run directory or initialize it in a view. `capability()` preserves the document authority unchanged, including read-only access; `session()` creates a separate window session sharing the registry. The compatible path constructor `EditorBackend::new(capability, private_journal_directory)` creates/opens its own approved root once and delegates to retained-root initialization. It is a new trusted grant, NOT a substitute for retaining a pre-existing DirectoryPlan proof. Proof consumers must use `new_anchored`, not extract a path and re-grant access.

`EditorSession::read(Location, EditorCancellation)` and `write(WriteText, EditorCancellation)` use the existing 192 KiB transfer operations and an 8 MiB text ceiling. Reads reject an oversized source before hashing or allocating its contents. Saves preserve the original revision/identity checks and atomic replacement. Cancellation is checked during inspection, chunks and commit; cancelled operations clean their identified stage. Inline `FileService::read_text` and `write_text` remain limited to 128 KiB. No NativeAction, generic IPC, image or form-field limit is widened.

Desktop composition retains one `EditorBackend` with its original local-account document authority and a separately prepared metadata root. The Files panel opens text through its shared sessions and opts only the asset-backed file field into 8 MiB. Local activation follows the closed focused source review. Peer documents still require their own bound backend/admission and cannot fall back to the local issuer. Saved immutable snapshots retain edits typed during an asynchronous save. Failed/cancelled loads retain the previous draft; reload uses the existing dirty confirmation. Close guards remain pending through cancellation cleanup. The field keeps shared per-edit text and compact byte/UTF-16/line checkpoints, paints visible rows and bounded UTF-8 long-line windows, and caches dirty comparisons by snapshot identity rather than cloning or comparing the whole buffer on every paint.

Large-document assistive text access is **not accepted** by this change. Until the separately reviewed full-document provider exists, asset editors above 128 KiB or 4096 rows expose an explicitly unavailable Group, not a partial snippet advertised as a full text pattern. Small-field text semantics, secret masking and stale-action guards remain. Native geometry, rendering performance and large-document AT acceptance need platform/window evidence; pure source tests do not prove them.

## Capability-scoped native metadata

`FileService::read_private_leaf(leaf, max)` returns bounded private bytes and a version from an AnchoredRelative native capability. It checks ordinary capability revocation before and after reading. Service must separately check actual ownership before and after the operation.

`Arc<FileService>::prepare_private_leaf_publication(destination, stage_leaf, expected)` returns `ScopedPrivatePublication` or typed `ScopedPrivatePreparationFailure`. Both retain the same capability and original creation custody; neither exposes a parent, raw File, path, issuer or inner publication authority. The failure wrapper is allocated before native creation, is not `std::error::Error`, and has no implicit error conversion. A missing retained attempt means preparation failed before creation or any retained native attempt.

`write_all` is bounded and one-shot. Partial/uncertain operations are never replayed. `publish` rechecks the same capability and inherited fences after prepublication validation, immediately before entering the namespace operation under the mutation guard. `ScopedPublishedReceipt` exposes diagnostic identity/version only, not a handle or constructible ownership proof.

`cleanup` uses only the original owned receipt and remains available after revocation. A poisoned mutation lock returns an explicit failure. `preserve_stale` remains usable without that lock or fresh write authority. Unix cleanup still preserves evidence without an accepted cross-process fence. These methods do not claim Go/filesystem atomicity, NoOwner, native journal readiness or new runtime permission.

`FileService::open_or_create_private_child(self: &Arc<Self>, leaf: &OsStr)` returns a narrower AnchoredRelative `Arc<FileService>` or `DirectoryPreparationFailure`. It admits only one direct child under the same native RW capability. Existing children are verified read-only, then explicitly reopened RW relative to the same retained parent with full identity equality. Existing privacy is never repaired, and a create collision is not retried as adoption. Missing children enter the original bounded preparation ledger before any fallible post-create clone or derivation. The returned capability inherits all fences, revocations and that ledger. Keep the original `DirectoryPreparationOwner` outside unwind and retain typed failures. No directory path, raw parent or account issuer is exported.

## Shared private native creation

The host-native `create_private_directory_at` / `create_private_file_at` functions accept the original held parent File and one validated leaf, not an absolute path or serialized authority. They return sealed `CreatedPrivate` on exclusive creation. `open_existing_private_at` returns separate `VerifiedPrivate` and never repairs an existing object. All caller logical RW/scope/revocation and namespace-custody guards remain required; the primitive does not grant them.

Windows creation supplies the complete protected current-user owner/DACL at `NtCreateFile(FILE_CREATE)`. Directory handles exclude DELETE and deny FILE_SHARE_DELETE while their actual pin is retained; other proof pins prevent cleanup until released. Regular stages and exclusive leases have separately specified sharing. Unix files request0600 exclusively at open; directories request0700 at mkdir and then acquire a no-follow handle under the stated trusted-parent assumption. POSIX mkdir does not return a handle, so this is not hostile-same-account or global Unix VFS confinement. Darwin unknown/nonempty ACL inheritance is an explicit unsupported gap.

Creation errors use `PrivateCreationFailure`, deliberately NOT `std::error::Error` and with no implicit FileError conversion. After native creation may have occurred, the same preallocated boxed payload retains original-parent/name/evidence and any actual returned FD/full identity. Owners reserve bounded slots BEFORE syscall and retain or explicitly preserve custody BEFORE mapping a diagnostic. A local issuer dropped through `?` is not adequate ownership. `into_parts` moves out the existing payload without creating a new cleanup scope.

Lease evidence is issued only after actual Unix flock or Windows exclusive sharing succeeds. Lease names are never automatically unlinked, including a create/lock race lost to another holder. Hardlink-count checks apply to files, not an invalid directory-nlink assumption. Directory cleanup borrows custody and explicitly releases its own pin; errors retain full identity/state. No implicit Drop unlink occurs.

`bounded_private_read_at` returns bounded bytes and an opaque full-identity content version. It is not an ownership receipt. Metadata publication instead uses the retained, pre-write `OwnedPrivatePublication` API documented in `docs/files-owned-publication-attempt.md`; the former consume-on-publish convenience helper was removed. Native rename errors are not replayed, known publication with uncertain durability is not NoWrite, and success keeps an actual opaque owned receipt. Unix cleanup without an accepted cross-process fence preserves bounded artifact evidence instead of check-then-unlink. No bool or read-derived token creates deletion authority.

The generic Windows File-based private-parent verifier still refuses unproven mutating ACLs, including the confirmed0x001301bf default-fixture case. The reviewed Windows DirectoryPlan production creator is a separate plan-bound operation using its original handles, full identities and creation ledger. It does not relax that verifier, repair existing roots or retag ordinary prefixes.

## V2 recovery and live stage custody

The new metadata file is `file-transfer-stages-v2.json`, version2, bounded to32 records and4MiB. The separate `.file-transfer-stages-v2.lock` uses real exclusive sharing or flock. Existing roots and leases are verified, never permission-repaired. V1 documents, old leases, old `.next` files and unknown V2 temporaries are left untouched.

A persisted path, UUID, revision or newly read identity is not stage ownership. Before user-file creation, the V2 journal installs a live owner containing the SAME user capability Arc, actual held parent/full identity and original creator slot. Windows native inputs/outputs live in that owner's stable allocation before entry. Successful native output enters the original FD slot before birth, identity, clone or authorization postchecks. Uncertain output is neither blindly adopted nor closed, and possibly OS-live inputs are retained within the bounded attempt budget. No user bytes are written without positive creation evidence.

Writes use the retained FD. Final publication checks current user authorization and the original parent, stage bytes and expected target version. Windows user CAS uses FileRenameInformationEx with POSIX replacement so the expected old destination FD stays alive. Private metadata/publication keep their existing rename semantics. Unix fsync opens literal '.' read-only relative to the original directory, checks full identity and syncs that same object, not an O_PATH lookup FD.

Live cancellation can dispose only its original creator object, including after capability revocation. Unknown publication/durability outcomes are preserved and never retried. On restart there is no live owner, so V2 records are diagnostic only. Startup does not open, adopt or delete their user paths. Legacy, forged, replaced and uncertain stages stay intact. A full preserved-record budget refuses new writes. Users can inspect drafts through a separately authorized ordinary file operation; the record itself grants no access or cleanup.

Current metadata-root privacy is checked per use, separately from the document capability. A private root alone does not authorize user-stage recovery. Unix user stages also require the existing non-mutating trusted-parent policy. Hostile same-account/admin filesystem races remain outside the accepted model.

Bounded inline text/image APIs keep their original limits and are not an editor/transfer fallback. They share the retained-destination user rename and same-object directory sync, and preserve artifacts after publication may have been entered. There is no automatic retry of an uncertain result. This is not a power-loss durability or native approval-store acceptance claim.

## Terminal upload inbox

`UploadInbox::open` uses an actual RW inbox capability plus a separate private `.shellfox-transfer-meta` V2 journal. Fresh UUID directories use the strict exclusive private creator. A collision is not adopted and existing permissions are never repaired. The receipt stays alive until the V2 stage captures its actual parent and original creator FD. The inbox's current private binding is checked per use.

`UploadActor` binds connection, terminal generation/control epoch and local/WSL environment. The host must validate live approval/control and the non-elevated profile at every operation and commit. Fresh finish retries use only the current live completion; WSL translation is followed by another check and never falls back to a host path.

Prior-session UUID artifacts, including apparently completed filenames and empty directories, are preserved and counted toward quota. Names, mtime and freshly read IDs do not recreate saved completion or TTL cleanup authority. Automatic reuse of an upload after host restart is unavailable until an accepted durable completion contract exists. Current-session saves retain their existing quota/expiry and exact-identity checks. No cleanup recursively removes an unexpected tree.

## Filesystem and race boundaries

Paths reject controls, URLs, traversal, Windows drive-relative paths, UNC/network/device paths, alternate streams and ambiguous Windows names. Components are opened separately with no-follow semantics and reparse checks. Files must be regular and are opened without following the final link. Unix reads are nonblocking so a raced FIFO cannot hang the host.

Filesystem operations use directory handles. Selected roots retain their identity and are never silently rebound. Reads and replacements compare device, inode/file ID, size, modification time and change time on opened handles. Windows uses native change time, not creation time. Saves validate SHA-256 revisions, stage identity and bytes, parent identity and destination state immediately before commit. This process serializes committing mutations across service instances.

Every empty stage receives owner-only permissions before its first byte, even under a broad parent ACL. Unix replacements retain owner/group when permitted, rwx mode and access ACL, but never copy setuid/setgid bits onto edited content. Windows replacements retain owner identity, DACL and protection flag. Read-only targets or permission-copy failures abort the save. Linux/Android and Apple no-overwrite publication uses `renameat_with(NOREPLACE)`; Windows uses handle-relative `NtSetInformationFile`. Unsupported filesystem guarantees fail rather than falling back to check-then-overwrite.

These checks do not provide cross-process compare-and-swap. An external writer can still change an entry in the final interval before atomic replacement/rename/delete. Operations do not follow a changed entry to an outside target. An opened Unix directory may also be renamed externally; authority attaches to its opened identity, not an immutable pathname. Do not advertise a hostile-same-account sandbox or substitute this for OS account isolation.

## Verification

Use private output directories and the owned process runners. Metadata and lint do not execute test bodies:

```sh
cargo check -p shellfox-files --tests --locked --offline
cargo clippy -p shellfox-files --tests --locked --offline -- -D warnings
cargo test -p shellfox-files --lib --test parity --no-run --locked --offline
```

Set an explicit lane-owned CARGO_TARGET_DIR before each command and refuse quarantined outputs. For execution, hash/copy the actual unit artifact, register its original process handle or gated fork/pidfd BEFORE Resume/exec, then observe that same handle/child through exit. A timeout is failure and uncertain exit retains fixtures.

Current evidence is versioned under repository tmp/e-files-v2-* and tmp/e-desktop-v2-proof-001. Windows and Linux actual tests cover8MiB editor/CAS/chunks, conflict/cancel/reopen, original FD custody, revocation, V1/V2 preservation, bounded inline/image consistency and generic transfers. Read the exact source manifest and process receipts for each checkpoint. Older failed rename/O_PATH runs remain failures, not accepted reruns.

Desktop metadata/lint is not GUI, pixel or native AT acceptance. Android/iOS/macOS runtime, WSL guest operations and authenticated remote adapter acceptance require their own platform evidence. Large-document assistive text access remains explicitly unavailable, not a partial full-text provider. No release or full-migration completion is claimed.
