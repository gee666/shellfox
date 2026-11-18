# Native host root security status

This describes in-progress source, not a cold-start acceptance result. Final review and platform tests are pending. The isolated root must not be a production Shellfox data directory.

## Ownership and preparation

Service inspects the original path before creation. Existing ancestors are opened one component at a time without following links. Their held directory identities remain available throughout startup.

One `OfflinePreparation` exists before permission helpers run. Service consumes it into an `OwnershipAttempt` after capturing `BoundOwnershipRoot` from a clone of the same held root file. Acquisition borrows the retained attempt. Failed transitions return the original custody. Successful acquisition moves that custody into `BootGuard`.

Preparation cleanup fences further admissions and waits for original children. Only the transport's sealed `NoOwnerProof` authorizes a clean pre-acquisition outcome. Errors, thread completion and endpoint absence are not cleanup proof.

Existing roots are verified read-only before acquisition. Service does not repair root or lock permissions before learning whether the root is occupied. The transport owns Go lock artifacts; Service does not chmod or rewrite their ACLs.

Original-chain retention is wired in source. Before any helper, Service registers clones of the original ancestor handles with preparation. Actual newly created directory handles are appended before a subsequent helper or binding; the initial chain is not replaced by a re-walk. Registered controllers share the same guard. Native boot passes the acquired lifetime guard into a Service field declared after Host, so it moves with deferred Service cleanup and drops after Host. This is lifetime retention, not filesystem authority or Unix confinement. Runtime proof and Host/VFS review remain pending.

## Creation and platform gates

Unix directory creation requests mode 0700 relative to the held parent. Regular-file verification opens relative to a held directory with NONBLOCK, NOFOLLOW and CLOEXEC before checking type, owner, link count and identity. Permission changes act on validated descriptors.

Native root and layout directories now use the shared Files private-creation primitive with the original held parent. The consumer retains the returned CreatedPrivate record and actual child handle; it does not reopen a path to obtain the created object. Existing children use VerifiedPrivate and remain verify-only. Exclusive-create collisions do not trigger adoption. Windows security is supplied explicitly at relative creation, rather than inherited and repaired. This is compiled consumer integration, not native acceptance.

Windows rename-denial proof remains gated until the directory-only sharing policy and composition tests are reviewed. Retaining a handle opened with FILE_SHARE_DELETE does not prevent rename. Approved directory proof handles must exclude DELETE access and deny FILE_SHARE_DELETE. Directory cleanup must release its own pin explicitly, retain parent/full identity and cleanup state on failure, and fail safely while any other pin remains. File-stage and lease policies are separate.

Endpoint metadata now uses the actual scoped Files owned-publication API on the retained native control-child capability. Its fixed slot retains preparation failures before error mapping or one-shot writing. Endpoint reads and writes remain bounded to 4096 bytes. No endpoint metadata absolute open, rename or removal fallback remains.

For ephemeral endpoint activation only, the original published receipt, complete synced bytes, exact current-binding/version read-back, actor activation ACK, retained listener/address and current ownership checks are required. Windows directory durability may remain explicitly Unconfirmed with a warning; it is not Durable. Preferences/journal durability policy is unchanged. Runtime acceptance remains gated.

Cleanup retains the SDK owner, closes its child explicitly and reports DeletePending or preserved evidence without claiming absence. Unix lacks an accepted cross-process deletion fence, so artifacts are preserved. Unix sockets are no longer unlinked by Drop or absolute cleanup; their handles close and bounded per-attempt evidence is retained. UUID names avoid same-name rebind collision, but stale accumulation across restarts is not bounded or accepted. Unix acceptance remains gated pending that separate audit.

The Go helper must enforce the agreed expected-root identity before mutation. Its implementation and verification are a separate gate. The Rust producer uses the strict versioned OS-tagged tuple, including full Windows FILE_ID_INFO128; unsupported identity providers have no 64-bit fallback.

## Native control-child binding

Native bootstrap now constructs one private LocalDesktop RW issuer after actual Go acquisition and before owned control children. Infallible context construction leaves Go custody inside BootGuard before any fallible binding. The binding uses the actual sealed directory receipts; existing ReadOnly receipts require authorized relative RW reopen and full-ID match before new RW provenance is accepted.

The Service-private NativeControlChild wraps only the derived anchored-relative FileService. It exposes no raw parent, path, account issuer or wire authority. Its capability clones retain the exact lost token, root lifetime and original creation owner through the native mutation fence. That fence can deny writes only; consumers retain current-ownership read checks. Cleanup adds revocation in the existing owned lifecycle, without an untracked watcher.

Still-path-based Go library state/config is separate and unaccepted. The Rust control capability does not anchor those paths. Native boot now stores Musa's retained remote envelope before borrowed initialization, passing the actual control child and original ownership. Errors keep that envelope in BootGuard for cleanup. Approval writes and native user-stage journals/uploads remain separately gated. Endpoint source integration is described separately above; no runtime activation or native acceptance is claimed.

## Typed local-only availability

The known static native approval-storage blocker now returns the typed LocalOnly setup outcome before optional remote child/inventory IO, listener, runtime or grants. The same acquired BootGuard starts exactly one ordinary local Service/Host. LocalBootStatus::ReadyLocalOnly { remote_unavailable: NativeApprovalStoragePending } is emitted only after actual local startup and endpoint activation, with no transient Ready.

HostEndpoint/AppHost/prestart and frontend consumers preserve that explicit reason. No remote control or observer factory is installed, and no empty approval success is manufactured. Ordinary root/ownership/auth/IO/post-effect errors remain fatal and retained; they are not converted into local-only. This is not cold fallback, release/reacquire or storage authority acceptance.

## ACL limitations

ACL inspection uses held descriptors through the transport policy. Access/default/inherited ACLs are rejected rather than treated as fixed by chmod.

On macOS, extended ACLs are an explicit limitation. This currently includes benign deny-only and common home-directory deny-delete ACLs. They are not classified as safe until native descriptor and inheritance behavior is proven. Mode/UID checks alone do not establish macOS support. ACL-free paths must still pass every other platform and lifecycle gate.

## Frontend separation

The default UI root is the Host root's sibling `<name>.desktop`. Explicit UI roots and the separate device root must not overlap the Host root or each other. Run directories and the editor journal live under the UI root.

The native Host root's seven-item artifact allowlist is unchanged. No desktop artifact allowance is required.

## Verification boundary

The current Windows-target Service library/test metadata graph passes `cargo check --locked --offline -p shellfox-service --lib --tests` after original-chain retention, the Service-field lifetime path and shared private-directory creation were integrated. The final typed availability Remote/Service/Desktop/CLI test metadata check passed in 6.04 seconds and matching strict --tests Clippy passed in 9.77 seconds on Windows. No tests were executed. Earlier foreign attachment-offer and observer-mailbox test callsites have been corrected by their owners. Metadata checking does not execute tests, link the Service-lib test executable, or typecheck Unix-only code.

The historical four pure test passes predate the final permission and custody changes. They are not final security validation. No actual-Go run or shared Service-lib test relink is authorized by this document. A private public-API runner still requires approval before implementation or execution.
