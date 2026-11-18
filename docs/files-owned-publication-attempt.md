# Owned private metadata publication

Status: compiled files-layer API with leaf tests. Not consumer activation, a control-root authority bridge, a V1 evidence migration or user-file broad-permission publication policy.

## Custody and API

`prepare_private_publication_at(parent, destination, stage_leaf, expected_version)` returns `Result<OwnedPrivatePublication, PrivatePreparationFailure>`. The caller reserves bounded retention capacity and holds its ORIGINAL scope/revocation/namespace custody before calling it. Names are single relative leaves; there is no absolute reopen or new cleanup verifier.

The opaque attempt retains the original parent/full identity and exclusively created private stage handle/full identity BEFORE payload writing. Preparation errors after native creation may have happened return the retained attempt. The error deliberately does not implement `std::error::Error`, so blanket anyhow `?` cannot hide it. The caller must inspect/retain it or explicitly preserve stale evidence.

- `write_all(&mut self, bytes)` is bounded to4MiB and one-shot. It marks attempted before IO. A partial write error keeps the handle and cannot be retried as a fresh write.
- `publish(&mut self)` borrows custody. It never consumes the only receipt.
- `published_receipt()` borrows the opaque `PublishedPrivate` only after known native publication success. It cannot be constructed from a read/version.
- `cleanup(&mut self)` reports its outcome/errors while retaining custody. No Drop unlink occurs.
- `preserve_stale(&mut self)` explicitly closes the child handle, retaining original-parent and artifact evidence. This is not unlink, NoOwner, NoPublication or proof that a Go process/resource stopped.

Before any consumer maps a failure to a diagnostic, its original owner must retain the attempt/failure in a pre-reserved bounded slot or make an explicit stale-preservation disposition. Dropping a local issuer/HostRoot through `?` is not retained custody.

## State meanings

Preparing, WriteAttempted, Ready, PreparationFailed, WriteFailed, NotPublished, RenameAttempted, PublishedDurabilityUnconfirmed, PublishedDurable, Uncertain, CleanupDeletePending, CleanupUncertain and PreservedStaleEvidence are explicit states.

NotPublished is issued only for checks that fail before the native rename call. A native rename error is conservatively Uncertain and is never replayed. A later read does not create a published ownership receipt.

Known native success is published even when subsequent directory synchronization fails. That outcome is PublishedDurabilityUnconfirmed, with the actual owned published FD/identity/version retained and the durability error available. Windows has no portable directory durability guarantee here and is not reported as confirmed. An invalid repeat operation is Refused, not reclassified as NoWrite.

## Cleanup limits

Windows cleanup checks the original created/published handle's full identity, owned content version and phase before exact-handle deletion. Unknown write contents, preparation uncertainty or rename uncertainty are preserved. A cleanup error retains the state/handle; a native deletion error is not silently retried or described as successful cleanup. DeletePending requires final handle close.

Unix has no accepted cross-process namespace deletion fence in this API. Cleanup therefore preserves artifact evidence; it does not perform check-then-unlink after Go/namespace ownership loss. A future deletion path requires a real accepted owner guard type, not a bool/closure or an inferred NoOwner condition. Directory creation's trusted-parent assumptions do not close Unix VFS or supply that fence.

Preserved evidence may coexist with a retained live child FD. Explicit stale preservation closes that FD, but does not certify that the pathname exists or an external process ended. Owners must budget both live custody and stale artifacts; repeated uncertain attempts cannot allocate unbounded files/records.

## Separate work

The public creator return-type flip to retained `PrivateCreationFailure` is coordinated with DirectoryPlan/native/frontend owners. No implicit conversion may discard it. The original logical authority owner must survive preparation failure, not just its displayed error.

The WindowsAtomicCreationAnchor prototype and unsafe-parent default support remain private/review-gated. Existing user-file mode/DACL restoration before publication is unchanged and needs the separate policy proposal. No native filesystem/platform, GUI or privacy-completion claim follows from leaf state tests.
