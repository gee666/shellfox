//! Only native-return capture is local. The ORIGINAL pre-registered ledger owns every outcome.
use super::*;
use crate::{
    CreationCustody, PrivateCreationEvidence, PrivateCreationFailure, PrivatePreservationState,
};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};

#[cfg(test)]
pub(super) fn capture(
    anchor: &WindowsAtomicCreationAnchor,
    _mutation: &std::sync::MutexGuard<'_, ()>,
) -> Result<usize> {
    capture_step(&anchor.pins, _mutation)
}
pub(super) fn capture_step(
    pins: &PlanPins,
    _mutation: &std::sync::MutexGuard<'_, ()>,
) -> Result<usize> {
    pins.check()?;
    let parent = pins.parent()?.try_clone()?.into_std_file();
    let held_parent = pins.parent()?.try_clone()?;
    let leaf = pins.leaf.clone();
    let mut custody = Box::new(CreationCustody {
        parent: parent.try_clone()?,
        parent_id: pins.parent_identity.clone(),
        leaf: leaf.clone(),
        kind: PrivateKind::Directory,
        file: None,
        identity: None,
        lease: false,
        pin: false,
        evidence: PrivateCreationEvidence::CreationMayHaveOccurred,
        preservation: PrivatePreservationState::LiveCustody,
    });
    let unwind_error = FileError::new(
        crate::ErrorCode::StorageFailed,
        "Native anchor capture unwound; original custody retained.",
    );
    let mut ledger = pins
        .authority
        .plan_creations
        .lock()
        .map_err(|_| FileError::conflict())?;
    ledger.reserve(1)?;
    pins.check()?;
    let attempt = ledger.begin(held_parent, pins.parent_identity.clone(), leaf);
    // Both custody and ledger guard are OUTSIDE this boundary, never dropped by raw capture unwind.
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        crate::private_native::create_retained(
            &parent,
            &pins.leaf,
            PrivateKind::Directory,
            PrivateFileUse::Stage,
            &mut custody.file,
        )?;
        custody.evidence = PrivateCreationEvidence::NativeHandleRetained;
        custody.pin = true; // actual successful FILE_CREATE directory noDELETE/share3 request
        let file = custody.file.as_ref().ok_or_else(FileError::denied)?;
        let identity = HandleIdentity::from_file(file)?;
        custody.identity = Some(identity.clone());
        Ok(identity)
    }));
    match outcome {
        Ok(Ok(identity)) => {
            let Some(file) = custody.file.take() else {
                return Err(ledger.retain_failure(
                    attempt,
                    PrivateCreationFailure {
                        error: FileError::denied(),
                        custody: Some(custody),
                    },
                ));
            };
            // Move the SAME native File, not a reopen/clone, before ANY postvalidation or fault hook.
            // Creator provenance ONLY from this actual successful private native FILE_CREATE.
            // This is not a public birth/current-private receipt; verification still follows.
            Ok(ledger.commit_created_with_origin(
                attempt,
                identity,
                Dir::from_std_file(file),
                crate::directory_plan::create::DirectoryCreatorOrigin::PrivateCreator,
            ))
        }
        Ok(Err(error)) => {
            if custody.file.is_some() {
                custody.evidence = PrivateCreationEvidence::NativeHandleRetained;
            }
            Err(ledger.retain_failure(
                attempt,
                PrivateCreationFailure {
                    error,
                    custody: Some(custody),
                },
            ))
        }
        Err(payload) => {
            if custody.file.is_some() {
                custody.evidence = PrivateCreationEvidence::NativeHandleRetained;
            }
            // Bookkeeping only: original row/parent/evidence and any returned FD survive unwind.
            drop(ledger.retain_failure(
                attempt,
                PrivateCreationFailure {
                    error: unwind_error,
                    custody: Some(custody),
                },
            ));
            drop(ledger);
            resume_unwind(payload)
        }
    }
}
