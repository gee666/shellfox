//! Minimal metadata helpers over the SAME retained parent, sealed stage and full identity.
use super::*;
use crate::content;
use std::io::{Seek, SeekFrom};

pub const PRIVATE_METADATA_MAX_BYTES: usize = 4 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivateVersion {
    identity: PrivateIdentity,
    size: u64,
    revision: String,
}
impl PrivateVersion {
    pub fn identity(&self) -> &PrivateIdentity {
        &self.identity
    }
    pub fn size(&self) -> u64 {
        self.size
    }
    pub fn revision(&self) -> &str {
        &self.revision
    }
}
pub struct PrivateRead {
    pub bytes: Vec<u8>,
    pub version: PrivateVersion,
}
pub(super) fn read_file(file: &File, max: usize) -> Result<PrivateRead> {
    if max == 0 || max > PRIVATE_METADATA_MAX_BYTES {
        return Err(FileError::new(
            ErrorCode::Validation,
            "Invalid private metadata byte limit.",
        ));
    }
    let identity = private_verify::object(file, PrivateKind::File)?;
    let mut handle = cap_std::fs::File::from_std(file.try_clone()?);
    handle.seek(SeekFrom::Start(0))?;
    // Existing bounded attributes/growth checks are supplemental, NEVER the native identity.
    let (bytes, _) = content::read(&mut handle, max)?;
    if !identity.matches(file)? {
        return Err(FileError::conflict());
    }
    let version = PrivateVersion {
        identity,
        size: bytes.len() as u64,
        revision: content::revision(&bytes),
    };
    Ok(PrivateRead { bytes, version })
}
pub fn bounded_private_read_at(parent: &File, name: &OsStr, max: usize) -> Result<PrivateRead> {
    let file = open_existing_private_at(parent, name, PrivateKind::File, PrivateAccess::ReadOnly)?;
    let result = read_file(file.as_file(), max)?;
    file.verify_binding()?;
    Ok(result)
}
