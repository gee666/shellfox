//! Cap-scoped owner. Cleanup uses ONLY original creation custody, never renewed cap authority.
use super::*;
use crate::{
    PrivateCleanupDisposition, HandleIdentity, PrivatePublicationPhase, PrivatePublishOutcome,
    PublishedPrivate,
};

pub struct ScopedPrivatePublication {
    pub(super) cap: Arc<FileService>,
    pub(super) attempt: Option<OwnedPrivatePublication>,
}
/// Borrowed diagnostics/version only. No handle, constructor or transferable ownership proof.
pub struct ScopedPublishedReceipt<'a>(&'a PublishedPrivate);
impl ScopedPublishedReceipt<'_> {
    pub fn identity(&self) -> &HandleIdentity {
        self.0.identity()
    }
    pub fn version(&self) -> &PrivateVersion {
        self.0.version()
    }
}
pub struct ScopedPrivatePreparationFailure {
    error: FileError,
    attempt: Option<Box<ScopedPrivatePublication>>,
}
impl ScopedPrivatePreparationFailure {
    pub(super) fn rejected(error: FileError) -> Self {
        Self {
            error,
            attempt: None,
        }
    }
    pub(super) fn retained(error: FileError, attempt: Box<ScopedPrivatePublication>) -> Self {
        if attempt.attempt.is_none() {
            // Raw preparation returns None ONLY before create/no retained native attempt.
            // Possible creation, even without a returned child FD, already carries custody.
            return Self::rejected(error);
        }
        Self {
            error,
            attempt: Some(attempt),
        }
    }
    pub fn error(&self) -> &FileError {
        &self.error
    }
    pub fn attempt(&self) -> Option<&ScopedPrivatePublication> {
        self.attempt.as_deref()
    }
    pub fn attempt_mut(&mut self) -> Option<&mut ScopedPrivatePublication> {
        self.attempt.as_deref_mut()
    }
    pub fn into_parts(self) -> (FileError, Option<ScopedPrivatePublication>) {
        (self.error, self.attempt.map(|value| *value))
    }
}
impl std::fmt::Debug for ScopedPrivatePreparationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScopedPrivatePreparationFailure")
            .field("error", &self.error)
            .field("retained_attempt", &self.attempt.is_some())
            .finish()
    }
}
impl std::fmt::Display for ScopedPrivatePreparationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}
// No Error/From implementation. A caller must explicitly retain or preserve failed creation custody.
fn state_error() -> FileError {
    FileError::new(
        ErrorCode::StorageFailed,
        "Missing retained private publication attempt.",
    )
}
impl ScopedPrivatePublication {
    pub fn phase(&self) -> PrivatePublicationPhase {
        self.attempt.as_ref().map_or(
            PrivatePublicationPhase::PreparationFailed,
            OwnedPrivatePublication::phase,
        )
    }
    /// Bounded one-shot write. Partial failures and replay remain governed by original owner state.
    pub fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
        let _mutation = FileService::mutation_lock()?;
        self.cap.authorized(true)?;
        let cap = &self.cap;
        self.attempt
            .as_mut()
            .ok_or_else(state_error)?
            .write_checked(bytes, || cap.authorized(true))?;
        self.cap.authorized(true)
    }
    pub fn publish(&mut self) -> PrivatePublishOutcome {
        let _mutation = match FileService::mutation_lock() {
            Ok(guard) => guard,
            Err(error) => return PrivatePublishOutcome::Refused(error),
        };
        if let Err(error) = self.cap.authorized(true) {
            return PrivatePublishOutcome::Refused(error);
        }
        let cap = &self.cap;
        match self.attempt.as_mut() {
            Some(attempt) => attempt.publish_checked(|| cap.authorized(true)),
            None => PrivatePublishOutcome::Refused(state_error()),
        }
    }
    pub fn cleanup(&mut self) -> Result<PrivateCleanupDisposition> {
        let _mutation = FileService::mutation_lock()?;
        self.attempt.as_mut().ok_or_else(state_error)?.cleanup()
    }
    pub fn preserve_stale(&mut self) -> PrivateCleanupDisposition {
        self.attempt.as_mut().map_or(
            PrivateCleanupDisposition::PreservedArtifactEvidence {
                live_file_retained: false,
            },
            OwnedPrivatePublication::preserve_stale,
        )
    }
    pub fn published_receipt(&self) -> Option<ScopedPublishedReceipt<'_>> {
        self.attempt
            .as_ref()?
            .published_receipt()
            .map(ScopedPublishedReceipt)
    }
    pub fn durability_error(&self) -> Option<&FileError> {
        self.attempt.as_ref()?.durability_error()
    }
    pub fn verify_retained_parent(&self) -> Result<()> {
        self.attempt
            .as_ref()
            .ok_or_else(state_error)?
            .verify_retained_parent()
    }
    pub fn stale_artifact(
        &self,
    ) -> Option<(&OsStr, Option<&HandleIdentity>, Option<&PrivateVersion>)> {
        self.attempt.as_ref()?.stale_artifact()
    }
}
