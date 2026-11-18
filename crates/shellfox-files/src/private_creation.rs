//! Trusted host-native primitives. No path-based authority, wire DTO, or permission repair.
//! Callers retain their ORIGINAL logical RW/scope/revocation and namespace custody guard.
use crate::{paths, private_native, private_verify, ErrorCode, FileError, PrivateIdentity, Result};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
};
#[path = "private_metadata.rs"]
mod metadata;
#[path = "private_publication.rs"]
mod publication;
#[path = "creation_retained.rs"]
mod retained;
pub use metadata::{
    bounded_private_read_at, PrivateRead, PrivateVersion, PRIVATE_METADATA_MAX_BYTES,
};
pub use publication::{
    prepare_private_publication_at, OwnedPrivatePublication, PrivateCleanupDisposition,
    PrivateDurability, PrivatePreparationFailure, PrivatePublicationPhase, PrivatePublishOutcome,
    PublishedPrivate,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateKind {
    Directory,
    File,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateFileUse {
    Stage,
    ExclusiveLease,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateAccess {
    ReadOnly,
    ReadWrite,
    ExclusiveLease,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateCleanupState {
    Owned,
    DirectoryPinReleased,
    DeletePending,
}

struct Held {
    parent: File,
    parent_id: PrivateIdentity,
    leaf: OsString,
    file: File,
    identity: PrivateIdentity,
    kind: PrivateKind,
    access: PrivateAccess,
    lease: bool,
    pin: bool,
    cleanup: PrivateCleanupState,
}
/// Issued only by successful exclusive creation, never by opening/repairing an existing object.
pub struct CreatedPrivate(Held);
/// Current-private verification only. It does NOT certify historical birth privacy.
pub struct VerifiedPrivate(Held);
/// Borrow selector only. The adapter still validates actual kind/RW provenance and original issuer.
#[derive(Clone, Copy)]
pub enum PreparedPrivateDirectoryRef<'a> {
    Created(&'a CreatedPrivate),
    Verified(&'a VerifiedPrivate),
}
#[path = "native_directory_receipt.rs"]
pub(crate) mod native_receipt;
macro_rules! held_methods {
    ($type:ident) => {
        impl $type {
            pub fn kind(&self) -> PrivateKind {
                self.0.kind
            }
            /// Sealed request role from the real factory, NOT inferred Unix FD access or a grant.
            pub fn requested_access(&self) -> PrivateAccess {
                self.0.access
            }
            pub fn as_file(&self) -> &File {
                &self.0.file
            }
            pub fn identity(&self) -> &PrivateIdentity {
                &self.0.identity
            }
            /// True only after Unix flock acquisition or successful Windows exclusive sharing.
            /// Delegating the returned File also delegates its lock lifetime; callers must not unlock it.
            pub fn holds_exclusive_lease(&self) -> bool {
                self.0.lease
            }
            pub fn directory_pin_is_held(&self) -> bool {
                self.0.pin
            }
            pub fn cleanup_state(&self) -> PrivateCleanupState {
                self.0.cleanup
            }
            pub fn verify_binding(&self) -> Result<()> {
                verify_binding(&self.0)
            }
            pub fn into_file(self) -> File {
                self.0.file
            }
        }
    };
}
held_methods!(CreatedPrivate);
held_methods!(VerifiedPrivate);
impl CreatedPrivate {
    /// Explicit owned cleanup only. Lease names are NEVER unlinked, even by their creator.
    pub fn remove_owned(&mut self) -> Result<()> {
        if self.0.lease {
            return Err(FileError::new(
                ErrorCode::Unsupported,
                "Lease names are retained; close the acquired handle to release the lease.",
            ));
        }
        if self.0.cleanup == PrivateCleanupState::DeletePending {
            return Ok(());
        }
        self.verify_binding()?;
        if self.0.pin {
            // End our rename-denial lifetime explicitly, retaining a same-object custody handle.
            let custody = private_native::open(
                &self.0.parent,
                &self.0.leaf,
                self.0.kind,
                PrivateAccess::ReadOnly,
            )?;
            if !self.0.identity.matches(&custody)? {
                return Err(FileError::conflict());
            }
            self.0.file = custody;
            self.0.pin = false;
            self.0.cleanup = PrivateCleanupState::DirectoryPinReleased;
        }
        private_native::remove(&self.0.parent, &self.0.leaf, &self.0.file, self.0.kind)?;
        self.0.cleanup = PrivateCleanupState::DeletePending;
        Ok(())
    }
}
fn leaf(name: &OsStr) -> Result<()> {
    let value = name.to_str().ok_or_else(FileError::denied)?;
    paths::validate(value)?;
    if value == "."
        || value == ".."
        || value.contains(['/', '\\', ':'])
        || std::path::Path::new(value).is_absolute()
    {
        return Err(FileError::denied());
    }
    Ok(())
}
fn verify_binding(value: &Held) -> Result<()> {
    if value.cleanup == PrivateCleanupState::DeletePending {
        return Err(FileError::conflict());
    }
    if !value.parent_id.matches(&value.parent)? || !value.identity.matches(&value.file)? {
        return Err(FileError::conflict());
    }
    private_verify::parent(&value.parent)?;
    #[cfg(windows)]
    if value.lease {
        // Share0 pins the named object against opens/rename/delete for this acquired handle.
        private_verify::object(&value.file, value.kind)?;
        return Ok(());
    }
    let named = private_native::open(
        &value.parent,
        &value.leaf,
        value.kind,
        PrivateAccess::ReadOnly,
    )?;
    if !value.identity.matches(&named)? {
        return Err(FileError::conflict());
    }
    private_verify::object(&named, value.kind)?;
    Ok(())
}
/// POSIX mkdir returns no handle. Trusted parent ownership/non-mutating ACLs and caller namespace
/// custody are required for the following no-follow open. This is NOT hostile-same-account or
/// global VFS confinement. Windows returns the kernel-created directory handle atomically.
pub fn create_private_directory_at(
    parent: &File,
    name: &OsStr,
) -> std::result::Result<CreatedPrivate, crate::PrivateCreationFailure> {
    retained::create(parent, name, PrivateKind::Directory, PrivateFileUse::Stage)
}
pub fn create_private_file_at(
    parent: &File,
    name: &OsStr,
    purpose: PrivateFileUse,
) -> std::result::Result<CreatedPrivate, crate::PrivateCreationFailure> {
    retained::create(parent, name, PrivateKind::File, purpose)
}
pub fn open_existing_private_at(
    parent: &File,
    name: &OsStr,
    kind: PrivateKind,
    access: PrivateAccess,
) -> Result<VerifiedPrivate> {
    leaf(name)?;
    if kind == PrivateKind::Directory && access == PrivateAccess::ExclusiveLease {
        return Err(FileError::denied());
    }
    let parent_id = private_verify::parent(parent)?;
    let held_parent = parent.try_clone()?;
    let file = private_native::open(parent, name, kind, access)?;
    let identity = private_verify::object(&file, kind)?;
    let lease = access == PrivateAccess::ExclusiveLease;
    if lease {
        private_native::lock(&file)?;
    }
    let result = VerifiedPrivate(Held {
        parent: held_parent,
        parent_id,
        leaf: name.to_owned(),
        file,
        identity,
        kind,
        access,
        lease,
        pin: false,
        cleanup: PrivateCleanupState::Owned,
    });
    result.verify_binding()?;
    Ok(result)
}

#[cfg(test)]
mod tests;
