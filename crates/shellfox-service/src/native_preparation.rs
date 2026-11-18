//! Original preparation stays installed through acquisition and shutdown.
use crate::security::PrivateDirectory;
use anyhow::{Context, Result};
use shellfox_transport::{
    AcquiredOwnershipView, BoundOwnershipRoot, NoOwnerProof, OfflinePreparation,
    OriginalCleanupView, OwnershipDecision,
};

#[derive(Debug)]
struct RetainedFilesystem {
    _owner: std::sync::Arc<crate::security::NativeCreationRetention>,
}
impl std::fmt::Display for RetainedFilesystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Native filesystem creation evidence retained, not a no-create proof")
    }
}
pub(super) fn retain_error(error: anyhow::Error, private: &PrivateDirectory) -> anyhow::Error {
    error.context(RetainedFilesystem {
        _owner: private.creation_owner(),
    })
}

pub(super) struct Preparation {
    original: OfflinePreparation,
    cleanup: OriginalCleanupView,
    bound: Option<BoundOwnershipRoot>,
    entered: bool,
}
impl Preparation {
    pub fn new(private: &PrivateDirectory) -> Result<Self> {
        let original = OfflinePreparation::new(private.helper.clone())?;
        let cleanup = original.original_cleanup_view();
        Ok(Self {
            original,
            cleanup,
            bound: None,
            entered: false,
        })
    }
    pub async fn acquire(
        &mut self,
        private: &mut PrivateDirectory,
        closing: &std::sync::atomic::AtomicBool,
    ) -> Result<OwnershipDecision> {
        anyhow::ensure!(!self.entered, "Native acquisition cannot be replayed");
        self.entered = true;
        private.prepare_root(&mut self.original, true).await?;
        anyhow::ensure!(
            !closing.load(std::sync::atomic::Ordering::Acquire),
            "Local host stop requested"
        );
        self.bound = Some(private.bound_root()?);
        self.original
            .bind_root_borrowed(self.bound.as_ref().context("Original bound root missing")?)?;
        Ok(self.original.acquire_borrowed().await?)
    }
    pub fn acquired_view(&self) -> Result<AcquiredOwnershipView<'_>> {
        Ok(self.original.acquired_view()?)
    }
    pub async fn cleanup(&self) -> Result<NoOwnerProof> {
        Ok(self.cleanup.shutdown_confirmed().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Debug)]
    struct Marker;
    impl std::fmt::Display for Marker {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("startup marker")
        }
    }
    impl std::error::Error for Marker {}
    #[test]
    fn retained_context_keeps_original_error_type_and_same_filesystem_owner() {
        let owner = std::sync::Arc::new(crate::security::NativeCreationRetention::default());
        let weak = std::sync::Arc::downgrade(&owner);
        let error = anyhow::Error::new(Marker).context(RetainedFilesystem { _owner: owner });
        assert!(error.is::<Marker>());
        assert!(weak.upgrade().is_some());
        drop(error);
        assert!(weak.upgrade().is_none());
    }
}
