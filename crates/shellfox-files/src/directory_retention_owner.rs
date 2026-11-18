//! Infallible original-issuer keeper remains usable after faults and poison.
use super::*;
/// Lifetime/custody access to the ORIGINAL issuer, not new filesystem authority.
/// Acquire before creation and keep outside catch_unwind alongside all plans.
#[derive(Clone)]
pub struct DirectoryPreparationOwner {
    pub(in crate::directory_plan::create) issuer: Arc<FileService>,
}
impl FileService {
    pub fn directory_preparation_owner(self: &Arc<Self>) -> DirectoryPreparationOwner {
        DirectoryPreparationOwner {
            issuer: self.clone(),
        }
    }
}
impl DirectoryPreparationOwner {
    pub fn retention(&self) -> Result<DirectoryRetention> {
        // A caught panic must not hide retained resources behind mutex poison.
        // Do not clear poison: ordinary creation stays fenced by its lock check.
        let ledger = self
            .issuer
            .plan_creations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(ledger.summary())
    }
    pub fn verify_retained(&self) -> Result<()> {
        self.issuer
            .plan_creations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .verify_retained()
    }
    /// Explicit non-unlink disposition; retain this owner and its parent/evidence.
    /// Neither a NoCreate/NoWrite/NoOwner claim nor permission to resume creation.
    pub fn preserve_stale(&mut self) -> Result<PrivatePreservationState> {
        {
            let mut ledger = self
                .issuer
                .plan_creations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            ledger.seal_preservation()?;
        }
        // Terminal seal is visible and ledger RELEASED before global MUTATIONS wait.
        let _mutation = crate::service::MUTATIONS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.issuer
            .plan_creations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .preserve_stale();
        Ok(PrivatePreservationState::StaleArtifactEvidence)
    }
}
impl std::fmt::Debug for DirectoryPreparationOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectoryPreparationOwner")
            .finish_non_exhaustive()
    }
}
/// Does not implement Error/From<FileError>: callers must keep the ORIGINAL
/// issuer/preparation owner or explicitly choose stale preservation.
pub struct DirectoryPreparationFailure {
    error: FileError,
    owner: DirectoryPreparationOwner,
}
impl std::fmt::Debug for DirectoryPreparationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectoryPreparationFailure")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Display for DirectoryPreparationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}
impl DirectoryPreparationFailure {
    pub(crate) fn new(error: FileError, issuer: Arc<FileService>) -> Self {
        Self {
            error,
            owner: issuer.directory_preparation_owner(),
        }
    }
    pub fn error(&self) -> &FileError {
        &self.error
    }
    pub fn retention(&self) -> Result<DirectoryRetention> {
        self.owner.retention()
    }
    pub fn verify_retained(&self) -> Result<()> {
        self.owner.verify_retained()
    }
    /// Explicit non-unlink disposition. Keep this owner: parent/evidence remains
    /// retained. This is not NoCreate/NoWrite/NoOwner or a native cleanup proof.
    pub fn preserve_stale(&mut self) -> Result<PrivatePreservationState> {
        self.owner.preserve_stale()
    }
}
