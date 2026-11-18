//! Sealed receipt borrowing for the native adapter; never a public File-to-RW constructor.
use super::*;
#[cfg(test)]
pub(crate) fn validate_leaf(name: &OsStr) -> Result<()> {
    leaf(name)
}
pub(crate) struct DirectoryReceipt<'a> {
    pub parent: &'a File,
    pub parent_id: &'a PrivateIdentity,
    pub leaf: &'a OsStr,
    pub file: &'a File,
    pub identity: &'a PrivateIdentity,
    pub access: PrivateAccess,
    #[cfg(windows)]
    pub pin: bool,
}
pub(crate) fn view(value: PreparedPrivateDirectoryRef<'_>) -> Result<DirectoryReceipt<'_>> {
    let held = match value {
        PreparedPrivateDirectoryRef::Created(value) => &value.0,
        PreparedPrivateDirectoryRef::Verified(value) => &value.0,
    };
    if held.kind != PrivateKind::Directory
        || held.cleanup != PrivateCleanupState::Owned
        || held.lease
    {
        return Err(FileError::denied());
    }
    verify_binding(held)?;
    private_verify::object(&held.file, PrivateKind::Directory)?;
    Ok(DirectoryReceipt {
        parent: &held.parent,
        parent_id: &held.parent_id,
        leaf: &held.leaf,
        file: &held.file,
        identity: &held.identity,
        access: held.access,
        #[cfg(windows)]
        pin: held.pin,
    })
}
/// Caller MUST hold the original native RW authority and same acquired-owner restrictions under
/// Files mutation discipline. Scoped callers also prove the receipt's exact direct-parent fullID.
/// This helper does not itself grant logical RW authority or infer it from directory FD flags.
pub(crate) fn reopen_rw(original: &VerifiedPrivate) -> Result<VerifiedPrivate> {
    let view = view(PreparedPrivateDirectoryRef::Verified(original))?;
    let parent = view.parent.try_clone()?;
    let file = private_native::open_rw_pinned_directory(&parent, view.leaf)?;
    let identity = private_verify::object(&file, PrivateKind::Directory)?;
    if &identity != view.identity || !view.parent_id.matches(&parent)? {
        return Err(FileError::conflict());
    }
    let value = VerifiedPrivate(Held {
        parent,
        parent_id: view.parent_id.clone(),
        leaf: view.leaf.to_owned(),
        file,
        identity,
        kind: PrivateKind::Directory,
        access: PrivateAccess::ReadWrite,
        lease: false,
        pin: cfg!(windows),
        cleanup: PrivateCleanupState::Owned,
    });
    value.verify_binding()?;
    Ok(value)
}
