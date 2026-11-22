//! Safe creation: the retained error payload is allocated BEFORE native mutation.
use super::*;
use crate::{
    CreationCustody, PrivateCreationEvidence, PrivateCreationFailure, PrivatePreservationState,
};
fn rejected(error: FileError) -> PrivateCreationFailure {
    PrivateCreationFailure {
        error,
        custody: None,
    }
}
fn failed(error: FileError, custody: Box<CreationCustody>) -> PrivateCreationFailure {
    PrivateCreationFailure {
        error,
        custody: Some(custody),
    }
}
fn binding(custody: &CreationCustody, app_directory: bool) -> Result<()> {
    let file = custody.file.as_ref().ok_or_else(FileError::denied)?;
    let identity = custody.identity.as_ref().ok_or_else(FileError::denied)?;
    if !custody.parent_id.matches(&custody.parent)? || !identity.matches(file)? {
        return Err(FileError::conflict());
    }
    private_verify::directory_parent(&custody.parent, app_directory)?;
    #[cfg(windows)]
    if custody.lease {
        return Ok(());
    } // acquired share0 protects the name
    let named = private_native::open(
        &custody.parent,
        &custody.leaf,
        custody.kind,
        PrivateAccess::ReadOnly,
    )?;
    if !identity.matches(&named)? {
        return Err(FileError::conflict());
    }
    private_verify::object(&named, custody.kind)?;
    Ok(())
}
pub(super) fn create(
    parent: &File,
    name: &OsStr,
    kind: PrivateKind,
    purpose: PrivateFileUse,
) -> std::result::Result<CreatedPrivate, PrivateCreationFailure> {
    create_inner(parent, name, kind, purpose, false, |_| Ok(()))
}
#[cfg(windows)]
pub(super) fn create_app_directory(
    parent: &File,
    name: &OsStr,
) -> std::result::Result<CreatedPrivate, PrivateCreationFailure> {
    create_inner(
        parent,
        name,
        PrivateKind::Directory,
        PrivateFileUse::Stage,
        true,
        |_| Ok(()),
    )
}
#[cfg(test)]
pub(super) fn create_with(
    parent: &File,
    name: &OsStr,
    kind: PrivateKind,
    purpose: PrivateFileUse,
    after_create: impl FnOnce(&File) -> Result<()>,
) -> std::result::Result<CreatedPrivate, PrivateCreationFailure> {
    create_inner(parent, name, kind, purpose, false, after_create)
}
fn create_inner(
    parent: &File,
    name: &OsStr,
    kind: PrivateKind,
    purpose: PrivateFileUse,
    app_directory: bool,
    after_create: impl FnOnce(&File) -> Result<()>,
) -> std::result::Result<CreatedPrivate, PrivateCreationFailure> {
    leaf(name).map_err(rejected)?;
    let parent_id = private_verify::directory_parent(parent, app_directory).map_err(rejected)?;
    let held_parent = parent.try_clone().map_err(|error| rejected(error.into()))?;
    // This box is the SAME allocation moved into an error. No allocation after a failed create.
    let mut custody = Box::new(CreationCustody {
        parent: held_parent,
        parent_id,
        leaf: name.to_owned(),
        kind,
        file: None,
        identity: None,
        lease: false,
        pin: false,
        evidence: PrivateCreationEvidence::CreationMayHaveOccurred,
        preservation: PrivatePreservationState::LiveCustody,
    });
    if let Err(error) =
        private_native::create_retained(parent, name, kind, purpose, &mut custody.file)
    {
        if custody.file.is_some() {
            custody.evidence = PrivateCreationEvidence::NativeHandleRetained;
        }
        return Err(failed(error, custody));
    }
    custody.evidence = PrivateCreationEvidence::NativeHandleRetained;
    custody.pin = cfg!(windows) && kind == PrivateKind::Directory;
    let file = match custody.file.as_ref() {
        Some(file) => file,
        None => {
            return Err(failed(
                FileError::new(
                    ErrorCode::StorageFailed,
                    "Native creation custody has no returned handle.",
                ),
                custody,
            ))
        }
    };
    let identity = match HandleIdentity::from_file(file) {
        Ok(id) => id,
        Err(error) => return Err(failed(error, custody)),
    };
    custody.identity = Some(identity);
    if let Err(error) = after_create(file) {
        return Err(failed(error, custody));
    }
    if let Err(error) = private_verify::object(file, kind) {
        return Err(failed(error, custody));
    }
    let lease = purpose == PrivateFileUse::ExclusiveLease;
    if lease {
        if let Err(error) = private_native::lock(file) {
            return Err(failed(error, custody));
        }
        custody.lease = true;
    }
    // Verify while every fallible step still owns the preallocated error payload intact.
    if let Err(error) = binding(&custody, app_directory) {
        return Err(failed(error, custody));
    }
    let Some(id) = custody.identity.clone() else {
        return Err(failed(FileError::denied(), custody));
    };
    let Some(file) = custody.file.take() else {
        return Err(failed(FileError::denied(), custody));
    };
    let custody = *custody;
    Ok(CreatedPrivate(Held {
        parent: custody.parent,
        parent_id: custody.parent_id,
        leaf: custody.leaf,
        file,
        identity: id,
        kind,
        access: if lease {
            PrivateAccess::ExclusiveLease
        } else {
            PrivateAccess::ReadWrite
        },
        lease,
        pin: custody.pin,
        cleanup: PrivateCleanupState::Owned,
        app_directory,
    }))
}
