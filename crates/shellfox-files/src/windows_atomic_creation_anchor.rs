//! Private Windows DirectoryPlan preparation using the original scope, pins and creation ledger.
//! Handoff verifies current privacy; the returned ApprovedRoot is scope, not continuing privacy.
//! Generic File-based parent policy is unchanged. No privileged/same-account/VFS/ReFS claim.
use super::*;
use crate::{DirectoryPreparationFailure, HandleIdentity, PrivateFileUse, PrivateKind};
#[path = "windows_atomic_creation_anchor_candidate.rs"]
mod candidate;
#[path = "windows_atomic_creation_anchor_capture.rs"]
mod capture;
#[path = "windows_atomic_creation_anchor_directory.rs"]
mod directory;
pub(super) use candidate::prepare_private_root;

struct PlanPins {
    authority: Arc<FileService>,
    ancestors: Vec<Dir>,
    identities: Vec<HandleIdentity>,
    parent_index: usize,
    parent_identity: HandleIdentity,
    leaf: OsString,
    configured: PathBuf,
}
#[cfg(test)]
struct WindowsAtomicCreationAnchor {
    pins: PlanPins,
}
/// CURRENT verified native custody only. Existing reopen never issues birth/created ownership.
struct PinnedPrivateDirectory {
    pins: PlanPins,
    directory: Dir,
    identity: HandleIdentity,
}
#[cfg(test)]
struct PreservedEvidence {
    parent: HandleIdentity,
    child: HandleIdentity,
    leaf: OsString,
}
#[cfg(test)]
impl WindowsAtomicCreationAnchor {
    fn from_plan(plan: &DirectoryPlan) -> Result<Self> {
        plan.authority.can_derive_root()?;
        plan.revalidate()?;
        if plan.original.missing.len() != 1 {
            return Err(FileError::new(
                crate::ErrorCode::Unsupported,
                "Prototype creation requires exactly one originally missing leaf.",
            ));
        }
        let parent = plan
            .original
            .dirs
            .len()
            .checked_sub(1)
            .ok_or_else(FileError::denied)?;
        Ok(Self {
            pins: PlanPins::retain(plan, parent, plan.original.missing[0].clone())?,
        })
    }
    // Guard and original DirectoryPreparationOwner/plan stay outside any caller catch_unwind.
    fn create(
        self,
        mutation: &std::sync::MutexGuard<'_, ()>,
    ) -> std::result::Result<PinnedPrivateDirectory, DirectoryPreparationFailure> {
        self.create_with(mutation, || Ok(()))
    }
    fn create_with(
        self,
        mutation: &std::sync::MutexGuard<'_, ()>,
        after_registered: impl FnOnce() -> Result<()>,
    ) -> std::result::Result<PinnedPrivateDirectory, DirectoryPreparationFailure> {
        let authority = self.pins.authority.clone();
        let result = (|| {
            let index = capture::capture(&self, mutation)?;
            // Actual native FD and fullID already belong to ORIGINAL ledger. No local-only keeper.
            after_registered()?;
            let directory = self
                .pins
                .authority
                .plan_creations
                .lock()
                .map_err(|_| FileError::conflict())?
                .clone_created(index)?;
            let identity = HandleIdentity::from_directory(&directory)?;
            let value = PinnedPrivateDirectory {
                pins: self.pins,
                directory,
                identity,
            };
            value.verify()?;
            Ok(value)
        })();
        result.map_err(|error| DirectoryPreparationFailure::new(error, authority))
    }
}
#[cfg(test)]
#[path = "windows_atomic_creation_anchor_tests.rs"]
mod tests;
