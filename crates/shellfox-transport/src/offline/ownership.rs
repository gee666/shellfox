use crate::{
    offline_control::OfflineControl, preparation::OfflinePreparation, BoundOwnershipRoot,
    HelperError, OwnershipDecision, VerifiedHelper,
};
use std::{path::Path, sync::Arc};
use tokio_util::sync::CancellationToken;

/// Acquired original controller and SAME preparation/root custody. No PID reopen.
pub struct HostOwnership {
    preparation: OfflinePreparation,
    _root: BoundOwnershipRoot,
    control: Arc<OfflineControl>,
}
impl HostOwnership {
    pub(crate) fn from_preparation(
        preparation: OfflinePreparation,
        root: BoundOwnershipRoot,
        control: Arc<OfflineControl>,
    ) -> Self {
        Self {
            preparation,
            _root: root,
            control,
        }
    }
    /// Compatibility captures a no-follow original descriptor and sends expected
    /// identity too. Service retained-root callers must use its original File via
    /// OfflinePreparation/BoundOwnershipRoot, not recapture after revalidation.
    pub async fn acquire(
        executable: &VerifiedHelper,
        directory: &Path,
    ) -> Result<Option<Self>, HelperError> {
        let mut preparation = OfflinePreparation::new(executable.clone())?;
        let root = BoundOwnershipRoot::capture_current(directory)?;
        preparation.retain_original_directories(root.original_files()?)?;
        preparation
            .permission_verifier()
            .check(root.path(), crate::PrivatePathKind::Directory)
            .await?;
        let mut attempt = preparation
            .bind_root(root)
            .map_err(|failure| failure.error)?;
        match attempt.acquire().await? {
            OwnershipDecision::Acquired => attempt
                .into_acquired()
                .map(Some)
                .map_err(|failure| failure.error),
            OwnershipDecision::Occupied(_) => Ok(None),
        }
    }
    pub fn original_cleanup_view(&self) -> crate::OriginalCleanupView {
        self.preparation.original_cleanup_view()
    }
    pub fn acquired_view(&self) -> Result<crate::AcquiredOwnershipView<'_>, HelperError> {
        self.preparation.acquired_view()
    }
    pub fn root_lifetime(&self) -> crate::RootLifetimeGuard {
        self.preparation.scope.root_lifetime()
    }
    pub fn permission_verifier(&self) -> &crate::PermissionVerifier {
        self.preparation.permission_verifier()
    }
    pub fn lost(&self) -> CancellationToken {
        self.control.lost()
    }
    pub fn is_alive(&self) -> bool {
        !self.control.lost().is_cancelled()
    }
    pub async fn shutdown_confirmed(&mut self) -> Result<(), HelperError> {
        self.preparation.shutdown_confirmed().await.map(|_| ())
    }
    pub async fn shutdown(mut self) -> Result<(), HelperError> {
        self.shutdown_confirmed().await
    }
}
impl Drop for HostOwnership {
    fn drop(&mut self) {
        self.preparation.scope.close();
    }
}
