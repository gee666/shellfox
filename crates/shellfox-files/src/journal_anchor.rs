//! Child of service: access to the original private root, without a pathname re-open.
use super::FileService;
use crate::{
    journal_private,
    paths::{self, Target},
    Access, CapabilityKind, FileError, HandleIdentity, Result,
};
use cap_std::fs::{Dir, File};
use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

pub(crate) struct JournalRoot {
    authority: Arc<FileService>,
    directory: Dir,
    security: File,
    private: Vec<u8>,
    identity: HandleIdentity,
}
impl JournalRoot {
    pub fn open_verified(authority: Arc<FileService>) -> Result<Self> {
        // This root authorizes V2 METADATA only. User stages require separate live custody.
        authority.require_path_resolution()?;
        if authority.kind != CapabilityKind::ApprovedRoot || authority.access != Access::ReadWrite {
            return Err(FileError::denied());
        }
        authority.authorized(true)?;
        let directory = authority
            .root
            .as_ref()
            .ok_or_else(FileError::denied)?
            .dir
            .try_clone()?;
        let metadata = directory.dir_metadata()?;
        paths::reject_link(&metadata)?;
        if !metadata.is_dir() {
            return Err(FileError::denied());
        }
        let identity = HandleIdentity::from_file(&directory.try_clone()?.into_std_file())?;
        let security = File::from_std(directory.try_clone()?.into_std_file());
        if !identity.matches(&security.try_clone()?.into_std())? {
            return Err(FileError::conflict());
        }
        let private = journal_private::fingerprint(&security)?;
        let result = Self {
            authority,
            directory,
            security,
            private,
            identity,
        };
        result.check()?;
        Ok(result)
    }
    pub fn check(&self) -> Result<()> {
        self.authority.require_path_resolution()?;
        self.authority.authorized(true)?;
        let root = self.authority.root.as_ref().ok_or_else(FileError::denied)?;
        let info = self.directory.dir_metadata()?;
        paths::reject_link(&info)?;
        if !self
            .identity
            .matches(&root.dir.try_clone()?.into_std_file())?
            || !self
                .identity
                .matches(&self.directory.try_clone()?.into_std_file())?
            || !self
                .identity
                .matches(&self.security.try_clone()?.into_std())?
            || journal_private::fingerprint(&self.security)? != self.private
        {
            return Err(FileError::denied());
        }
        self.authority.authorized(true)
    }
    pub fn target(&self, name: &str) -> Result<Target> {
        self.check()?;
        paths::validate(name)?;
        let mut components = Path::new(name).components();
        if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
            return Err(FileError::denied());
        }
        Ok(Target {
            parent: self.directory.try_clone()?,
            name: PathBuf::from(name),
            // Diagnostic only. V2 never reopens recorded user-stage paths.
            absolute: self
                .authority
                .root_path()
                .ok_or_else(FileError::denied)?
                .join(name),
        })
    }
    pub fn directory(&self) -> Result<Dir> {
        self.check()?;
        Ok(self.directory.try_clone()?)
    }
}

#[cfg(test)]
#[path = "journal_anchor_tests.rs"]
mod tests;
