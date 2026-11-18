//! Trusted native metadata only. No root path, parent FD or inner authority escapes.
use super::*;
use crate::{
    bounded_private_read_at, prepare_private_publication_at, OwnedPrivatePublication, PrivateRead,
    PrivateVersion,
};
use std::ffi::OsStr;
#[path = "cap_private_child.rs"]
mod child;
#[cfg(test)]
#[path = "private_root_check_candidate.rs"]
pub(crate) mod current_private_candidate;
#[path = "cap_private_publication.rs"]
mod publication;
pub use publication::{
    ScopedPrivatePreparationFailure, ScopedPrivatePublication, ScopedPublishedReceipt,
};

fn unsupported() -> FileError {
    FileError::new(
        ErrorCode::Unsupported,
        "Private leaf metadata requires anchored-relative native authority.",
    )
}
impl FileService {
    /// Bounded private bytes/version, not ownership evidence. Service separately admits ownership.
    pub fn read_private_leaf(&self, leaf: &OsStr, max: usize) -> Result<PrivateRead> {
        self.authorized(false)?;
        if !self.is_anchored_relative() {
            return Err(unsupported());
        }
        let directory = self
            .root
            .as_ref()
            .ok_or_else(FileError::denied)?
            .dir
            .try_clone()?
            .into_std_file();
        let read = bounded_private_read_at(&directory, leaf, max)?;
        self.authorized(false)?;
        Ok(read)
    }
    /// Caller reserves retention before preparing. The returned failure must be handled explicitly.
    pub fn prepare_private_leaf_publication(
        self: &Arc<Self>,
        destination: &OsStr,
        stage_leaf: &OsStr,
        expected: Option<&PrivateVersion>,
    ) -> std::result::Result<ScopedPrivatePublication, ScopedPrivatePreparationFailure> {
        if !self.is_anchored_relative() {
            return Err(ScopedPrivatePreparationFailure::rejected(unsupported()));
        }
        let _mutation = Self::mutation_lock().map_err(ScopedPrivatePreparationFailure::rejected)?;
        let directory = self
            .retained_anchored_directory()
            .map_err(ScopedPrivatePreparationFailure::rejected)?
            .into_std_file();
        // Allocate BOTH the cap-retaining failure box and raw helper's retention before native create.
        let mut retained = Box::new(ScopedPrivatePublication {
            cap: self.clone(),
            attempt: None,
        });
        match prepare_private_publication_at(&directory, destination, stage_leaf, expected) {
            Ok(attempt) => {
                retained.attempt = Some(attempt);
                Ok(*retained)
            }
            Err(failure) => {
                let (error, attempt) = failure.into_parts();
                retained.attempt = attempt;
                Err(ScopedPrivatePreparationFailure::retained(error, retained))
            }
        }
    }
}
