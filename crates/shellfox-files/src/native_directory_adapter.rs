//! Child of service: native authority narrows a bootstrap-private account issuer to SEALED roots.
//! No path regrant, arbitrary-File constructor, cleanup fence or transport dependency.
use super::FileService;
use crate::{
    private_creation::native_receipt, Access, CapabilityKind, CreatedPrivate, ErrorCode, FileError,
    HandleIdentity, NativeCommitFence, PreparedPrivateDirectoryRef, PrivateAccess, Result,
    VerifiedPrivate,
};
use cap_std::fs::Dir;
use std::sync::Arc;

pub struct NativeChildIssuer {
    original: Arc<FileService>,
    parent: Arc<FileService>,
    parent_id: HandleIdentity,
    fence: Arc<dyn NativeCommitFence>,
}
#[cfg(test)]
#[path = "native_directory_adapter_tests.rs"]
mod tests;

fn issuer(value: &FileService) -> Result<()> {
    if value.kind != CapabilityKind::LocalDesktop {
        return Err(FileError::new(ErrorCode::Unsupported,"Native bootstrap requires its original local account issuer, not a peer or unproved selected root."));
    }
    if value.access != Access::ReadWrite {
        return Err(FileError::denied());
    }
    value.can_derive_root()
}
fn directory(
    value: PreparedPrivateDirectoryRef<'_>,
) -> Result<native_receipt::DirectoryReceipt<'_>> {
    let value = native_receipt::view(value)?;
    if value.access != PrivateAccess::ReadWrite {
        return Err(FileError::new(
            ErrorCode::AccessDenied,
            "Native directory receipt requires a separately authorized RW reopen.",
        ));
    }
    #[cfg(windows)]
    if !value.pin {
        return Err(FileError::new(
            ErrorCode::AccessDenied,
            "Native directory receipt lacks its actual no-delete-sharing pin.",
        ));
    }
    Ok(value)
}
impl FileService {
    /// Construct ONCE in trusted bootstrap AFTER actual native acquire, never on a wire request.
    pub fn native_child_issuer(
        self: &Arc<Self>,
        parent: PreparedPrivateDirectoryRef<'_>,
        fence: Arc<dyn NativeCommitFence>,
    ) -> Result<NativeChildIssuer> {
        issuer(self)?;
        let _mutation = Self::mutation_lock()?;
        issuer(self)?;
        fence.check()?;
        let receipt = directory(parent)?;
        let dir = Dir::from_std_file(receipt.file.try_clone()?);
        if HandleIdentity::from_directory(&dir)? != *receipt.identity {
            return Err(FileError::conflict());
        }
        let root =
            self.derive_anchored_relative(dir, "native-prepared-parent".into(), fence.clone())?;
        fence.check()?;
        self.authorized(true)?;
        Ok(NativeChildIssuer {
            original: self.clone(),
            parent: Arc::new(root),
            parent_id: receipt.identity.clone(),
            fence,
        })
    }
    /// Explicit authorization, not RO relabelling: same retained parent/leaf, fullID equality,
    /// actual RW requested provenance and Windows noDELETE/share3 pin are newly acquired.
    pub fn reopen_native_directory_rw(
        self: &Arc<Self>,
        original: &VerifiedPrivate,
        fence: Arc<dyn NativeCommitFence>,
    ) -> Result<VerifiedPrivate> {
        issuer(self)?;
        let _mutation = Self::mutation_lock()?;
        issuer(self)?;
        fence.check()?;
        let reopened = native_receipt::reopen_rw(original)?;
        if reopened.identity() != original.identity() {
            return Err(FileError::conflict());
        }
        fence.check()?;
        self.authorized(true)?;
        Ok(reopened)
    }
}
impl NativeChildIssuer {
    /// Trusted bootstrap only. Do not serialize/export this issuer or its original account grant.
    pub fn parent_root(&self) -> Arc<FileService> {
        self.parent.clone()
    }
    pub fn created_directory_root(&self, child: &CreatedPrivate) -> Result<Arc<FileService>> {
        self.child(PreparedPrivateDirectoryRef::Created(child))
    }
    pub fn verified_directory_root(&self, child: &VerifiedPrivate) -> Result<Arc<FileService>> {
        self.child(PreparedPrivateDirectoryRef::Verified(child))
    }
    fn child(&self, child: PreparedPrivateDirectoryRef<'_>) -> Result<Arc<FileService>> {
        self.original.authorized(true)?;
        let _mutation = FileService::mutation_lock()?;
        self.original.authorized(true)?;
        self.parent.can_derive_root()?;
        self.fence.check()?;
        let parent = self.parent.retained_anchored_directory()?;
        let identity = HandleIdentity::from_directory(&parent)?;
        if identity != self.parent_id {
            return Err(FileError::conflict());
        }
        let receipt = directory(child)?;
        if *receipt.parent_id != self.parent_id || !self.parent_id.matches(receipt.parent)? {
            return Err(FileError::denied());
        }
        let dir = Dir::from_std_file(receipt.file.try_clone()?);
        if HandleIdentity::from_directory(&dir)? != *receipt.identity {
            return Err(FileError::conflict());
        }
        let root = self.parent.derive_anchored_relative(
            dir,
            "native-prepared-child".into(),
            self.fence.clone(),
        )?;
        self.fence.check()?;
        self.original.authorized(true)?;
        Ok(Arc::new(root))
    }
}
