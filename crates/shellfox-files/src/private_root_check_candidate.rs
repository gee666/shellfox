//! STAGED current-private metadata checks. Not a journal/recovery authority or birth receipt.
use super::*;
use crate::{bounded_private_read_at, DirectoryPlan, HandleIdentity, PrivateKind, PrivateRead};
use std::ffi::OsStr;
#[derive(Clone, Copy)]
pub(crate) enum PrivateUse {
    Read,
    Stage,
    Write,
    Commit,
}
pub(crate) struct PrivateRootCheck {
    authority: Arc<FileService>,
    binding: DirectoryPlan,
    directory: Dir,
    identity: HandleIdentity,
}
impl PrivateRootCheck {
    pub(crate) fn verify(authority: Arc<FileService>) -> Result<Self> {
        authority.authorized(false)?;
        if authority.capability_kind() != CapabilityKind::ApprovedRoot {
            return Err(FileError::denied());
        }
        if authority.is_anchored_relative() {
            return Err(FileError::new(ErrorCode::Unsupported,"Native private metadata requires the original sealed parent binding; diagnostic labels cannot supply it."));
        }
        // The existing bounded no-follow planner verifies original spelling against held Root.
        // This is binding verification, never a path-derived authority regrant/canonicalization.
        let binding = authority.plan_directory(&Location { path: ".".into() })?;
        let directory = authority
            .root
            .as_ref()
            .ok_or_else(FileError::denied)?
            .dir
            .try_clone()?;
        let identity = HandleIdentity::from_directory(&directory)?;
        let value = Self {
            authority,
            binding,
            directory,
            identity,
        };
        value.check(PrivateUse::Read)?;
        Ok(value)
    }
    pub(crate) fn check(&self, use_kind: PrivateUse) -> Result<()> {
        let write = !matches!(use_kind, PrivateUse::Read);
        self.authority.authorized(write)?;
        self.binding.revalidate()?;
        if HandleIdentity::from_directory(&self.directory)? != self.identity {
            return Err(FileError::conflict());
        }
        let original = self.authority.root.as_ref().ok_or_else(FileError::denied)?;
        if HandleIdentity::from_directory(&original.dir)? != self.identity {
            return Err(FileError::conflict());
        }
        if crate::private_verify::object(
            &self.directory.try_clone()?.into_std_file(),
            PrivateKind::Directory,
        )? != self.identity
        {
            return Err(FileError::conflict());
        }
        self.authority.authorized(write)
    }
    pub(crate) fn read_optional_leaf(
        &self,
        leaf: &OsStr,
        max: usize,
    ) -> Result<Option<PrivateRead>> {
        crate::private_creation::native_receipt::validate_leaf(leaf)?;
        if max == 0 || max > crate::PRIVATE_METADATA_MAX_BYTES {
            return Err(FileError::new(
                ErrorCode::Validation,
                "Invalid private metadata byte limit.",
            ));
        }
        self.check(PrivateUse::Read)?;
        // Metadata is used ONLY to distinguish initial proven absence, not as identity evidence.
        let result = match self.directory.symlink_metadata(leaf) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
            Ok(_) => {
                bounded_private_read_at(&self.directory.try_clone()?.into_std_file(), leaf, max)
                    .map(Some)
            }
        };
        // A later NotFound from a started/opened read remains an ERROR, not missing/default data.
        self.check(PrivateUse::Read)?;
        result
    }
}
