//! One retained owner before write; publish/cleanup never consume its only custody.
use super::metadata::{read_file, PrivateVersion, PRIVATE_METADATA_MAX_BYTES};
use super::*;
use crate::CreationCustody;
#[path = "private_publication_io.rs"]
mod io;
#[path = "private_publication_write.rs"]
mod write;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivatePublicationPhase {
    Preparing,
    WriteAttempted,
    Ready,
    PreparationFailed,
    WriteFailed,
    NotPublished,
    RenameAttempted,
    PublishedDurabilityUnconfirmed,
    PublishedDurable,
    Uncertain,
    CleanupDeletePending,
    CleanupUncertain,
    PreservedStaleEvidence,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateDurability {
    Confirmed,
    Unconfirmed,
}
#[derive(Debug)]
pub enum PrivatePublishOutcome {
    NotPublished(FileError),
    Published(PrivateDurability),
    Uncertain(FileError),
    Refused(FileError),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivateCleanupDisposition {
    DeletePending,
    PreservedArtifactEvidence { live_file_retained: bool },
}
/// Only successful publication of the retained CREATED object can issue this receipt.
pub struct PublishedPrivate {
    owned: CreatedPrivate,
    version: PrivateVersion,
}
impl PublishedPrivate {
    #[cfg(test)]
    pub fn as_file(&self) -> &File {
        self.owned.as_file()
    }
    pub fn identity(&self) -> &PrivateIdentity {
        self.owned.identity()
    }
    pub fn version(&self) -> &PrivateVersion {
        &self.version
    }
}
#[cfg(test)]
#[path = "private_creation/publication_tests.rs"]
mod tests;

struct PublishInstall {
    version: Option<PrivateVersion>,
    destination: OsString,
}
struct StaleEvidence {
    parent: File,
    parent_id: PrivateIdentity,
    leaf: OsString,
    identity: Option<PrivateIdentity>,
    version: Option<PrivateVersion>,
}
pub struct OwnedPrivatePublication {
    parent: File,
    parent_id: PrivateIdentity,
    destination: OsString,
    expected: Option<PrivateVersion>,
    stage: Option<CreatedPrivate>,
    failed_creation: Option<CreationCustody>,
    published: Option<PublishedPrivate>,
    owned_version: Option<PrivateVersion>,
    stale: Option<StaleEvidence>,
    phase: PrivatePublicationPhase,
    durability_error: Option<FileError>,
}
pub struct PrivatePreparationFailure {
    error: FileError,
    attempt: Option<Box<OwnedPrivatePublication>>,
}
impl PrivatePreparationFailure {
    #[cfg(test)]
    pub fn attempt(&self) -> Option<&OwnedPrivatePublication> {
        self.attempt.as_deref()
    }
    pub fn into_parts(self) -> (FileError, Option<OwnedPrivatePublication>) {
        (self.error, self.attempt.map(|attempt| *attempt))
    }
}
impl std::fmt::Debug for PrivatePreparationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrivatePreparationFailure")
            .field("error", &self.error)
            .field("retained_attempt", &self.attempt.is_some())
            .finish()
    }
}
impl std::fmt::Display for PrivatePreparationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}
// Deliberately NOT std::error::Error: callers must retain/preserve the owned attempt explicitly.
fn state_error() -> FileError {
    FileError::new(
        ErrorCode::StorageFailed,
        "Private publication state forbids this operation; no uncertain mutation is retried.",
    )
}
fn failed(
    error: FileError,
    attempt: Option<Box<OwnedPrivatePublication>>,
) -> PrivatePreparationFailure {
    PrivatePreparationFailure { error, attempt }
}
/// Caller reserves bounded retention BEFORE this call and keeps its original authority/custody.
pub fn prepare_private_publication_at(
    parent: &File,
    destination: &OsStr,
    stage_name: &OsStr,
    expected: Option<&PrivateVersion>,
) -> std::result::Result<OwnedPrivatePublication, PrivatePreparationFailure> {
    leaf(destination).map_err(|error| failed(error, None))?;
    leaf(stage_name).map_err(|error| failed(error, None))?;
    if destination == stage_name {
        return Err(failed(FileError::denied(), None));
    }
    let parent_id = private_verify::parent(parent).map_err(|error| failed(error, None))?;
    let held = parent
        .try_clone()
        .map_err(|error| failed(error.into(), None))?;
    // Allocate retained error payload BEFORE any native create; errors move this same box.
    let mut value = Box::new(OwnedPrivatePublication {
        parent: held,
        parent_id,
        destination: destination.to_owned(),
        expected: expected.cloned(),
        stage: None,
        failed_creation: None,
        published: None,
        owned_version: None,
        stale: None,
        phase: PrivatePublicationPhase::Preparing,
        durability_error: None,
    });
    match super::retained::create(parent, stage_name, PrivateKind::File, PrivateFileUse::Stage) {
        Ok(stage) => value.stage = Some(stage),
        Err(failure) => {
            let (error, custody) = failure.into_parts();
            if custody.is_some() {
                value.failed_creation = custody;
                value.phase = PrivatePublicationPhase::PreparationFailed;
                return Err(failed(error, Some(value)));
            }
            return Err(failed(error, None));
        }
    }
    let initial = value
        .stage
        .as_ref()
        .map(|stage| read_file(stage.as_file(), PRIVATE_METADATA_MAX_BYTES));
    match initial {
        Some(Ok(read)) => value.owned_version = Some(read.version),
        Some(Err(error)) => {
            value.phase = PrivatePublicationPhase::PreparationFailed;
            return Err(failed(error, Some(value)));
        }
        None => {
            value.phase = PrivatePublicationPhase::PreparationFailed;
            return Err(failed(state_error(), Some(value)));
        }
    }
    Ok(*value)
}
impl OwnedPrivatePublication {
    pub fn phase(&self) -> PrivatePublicationPhase {
        self.phase
    }
    pub fn published_receipt(&self) -> Option<&PublishedPrivate> {
        self.published.as_ref()
    }
    pub fn durability_error(&self) -> Option<&FileError> {
        self.durability_error.as_ref()
    }
    #[cfg(test)]
    pub fn publish(&mut self) -> PrivatePublishOutcome {
        self.publish_checked(|| Ok(()))
    }
    pub(crate) fn publish_checked(
        &mut self,
        check: impl Fn() -> Result<()>,
    ) -> PrivatePublishOutcome {
        self.publish_with_ops_checked(
            |parent, stage, destination, file, replace| {
                io::rename_checked(parent, stage, destination, file, replace, &check)
            },
            |parent| {
                #[cfg(unix)]
                parent.sync_all()?;
                #[cfg(windows)]
                let _ = parent;
                Ok(())
            },
            &check,
        )
    }
    #[cfg(test)]
    fn publish_with_sync(
        &mut self,
        sync: impl FnOnce(&File) -> Result<()>,
    ) -> PrivatePublishOutcome {
        self.publish_with_ops(io::rename, sync)
    }
    #[cfg(test)]
    fn publish_with_ops(
        &mut self,
        rename: impl FnOnce(&File, &OsStr, &OsStr, &File, bool) -> Result<()>,
        sync: impl FnOnce(&File) -> Result<()>,
    ) -> PrivatePublishOutcome {
        self.publish_with_ops_checked(rename, sync, || Ok(()))
    }
    fn publish_with_ops_checked(
        &mut self,
        rename: impl FnOnce(&File, &OsStr, &OsStr, &File, bool) -> Result<()>,
        sync: impl FnOnce(&File) -> Result<()>,
        check: impl Fn() -> Result<()>,
    ) -> PrivatePublishOutcome {
        if self.phase != PrivatePublicationPhase::Ready {
            return PrivatePublishOutcome::Refused(state_error());
        }
        // Allocate while the actual FD is still in its original stage field, before rename.
        let install = PublishInstall {
            version: self.owned_version.clone(),
            destination: self.destination.clone(),
        };
        if let Err(error) = io::prepublish(self).and_then(|()| check()) {
            self.phase = PrivatePublicationPhase::NotPublished;
            return PrivatePublishOutcome::NotPublished(error);
        }
        self.phase = PrivatePublicationPhase::RenameAttempted;
        let Some(stage) = &self.stage else {
            self.phase = PrivatePublicationPhase::Uncertain;
            return PrivatePublishOutcome::Uncertain(state_error());
        };
        if let Err(error) = rename(
            &self.parent,
            &stage.0.leaf,
            &self.destination,
            stage.as_file(),
            self.expected.is_some(),
        ) {
            self.phase = PrivatePublicationPhase::Uncertain;
            return PrivatePublishOutcome::Uncertain(error);
        }
        self.phase = PrivatePublicationPhase::PublishedDurabilityUnconfirmed;
        let PublishInstall {
            version,
            mut destination,
        } = install;
        let Some(version) = version else {
            self.durability_error = Some(state_error());
            return PrivatePublishOutcome::Published(PrivateDurability::Unconfirmed);
        };
        let Some(mut owned) = self.stage.take() else {
            self.durability_error = Some(state_error());
            return PrivatePublishOutcome::Published(PrivateDurability::Unconfirmed);
        };
        std::mem::swap(&mut owned.0.leaf, &mut destination);
        self.published = Some(PublishedPrivate { owned, version });
        self.phase = PrivatePublicationPhase::PublishedDurabilityUnconfirmed;
        if let Err(error) = sync(&self.parent) {
            self.durability_error = Some(error);
            return PrivatePublishOutcome::Published(PrivateDurability::Unconfirmed);
        }
        #[cfg(unix)]
        {
            self.phase = PrivatePublicationPhase::PublishedDurable;
            PrivatePublishOutcome::Published(PrivateDurability::Confirmed)
        }
        #[cfg(windows)]
        {
            PrivatePublishOutcome::Published(PrivateDurability::Unconfirmed)
        }
    }
    /// Without an actual accepted cross-process deletion fence Unix only preserves evidence.
    /// This does not prove a Go owner/process/resource has stopped.
    pub fn cleanup(&mut self) -> Result<PrivateCleanupDisposition> {
        io::cleanup(self)
    }
    /// Explicitly release this child FD, not its name, retaining original-parent artifact evidence.
    pub fn preserve_stale(&mut self) -> PrivateCleanupDisposition {
        let version = self.owned_version.clone(); // before taking ownership or closing raw capture
        if let Some(value) = self.failed_creation.as_mut() {
            value.preserve_stale();
        }
        if let Some(owned) = self
            .stage
            .take()
            .or_else(|| self.published.take().map(|value| value.owned))
        {
            self.stale = Some(StaleEvidence {
                parent: owned.0.parent,
                parent_id: owned.0.parent_id,
                leaf: owned.0.leaf,
                identity: Some(owned.0.identity),
                version,
            });
            drop(owned.0.file);
        }
        self.phase = PrivatePublicationPhase::PreservedStaleEvidence;
        PrivateCleanupDisposition::PreservedArtifactEvidence {
            live_file_retained: false,
        }
    }
    pub fn verify_retained_parent(&self) -> Result<()> {
        if !self.parent_id.matches(&self.parent)? {
            return Err(FileError::conflict());
        }
        if let Some(stale) = &self.stale {
            if !stale.parent_id.matches(&stale.parent)? {
                return Err(FileError::conflict());
            }
        }
        Ok(())
    }
    pub fn stale_artifact(
        &self,
    ) -> Option<(&OsStr, Option<&PrivateIdentity>, Option<&PrivateVersion>)> {
        self.stale.as_ref().map(|value| {
            (
                value.leaf.as_os_str(),
                value.identity.as_ref(),
                value.version.as_ref(),
            )
        })
    }
}
