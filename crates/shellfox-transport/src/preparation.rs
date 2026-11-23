use crate::{
    offline::OfflineCommand, offline_scope::OfflineScope, AcquiredOwnershipView,
    BoundOwnershipRoot, HelperError, HostOwnership, OriginalCleanupView, PermissionVerifier,
    VerifiedHelper,
};
use serde::Deserialize;
use std::{path::Path, time::Duration};

/// Sealed transport admission and resolved ORIGINAL children only. Not evidence
/// of Service/SQLite shutdown, publication disposition or filesystem confinement.
#[derive(Debug)]
pub struct NoOwnerProof {
    _sealed: (),
}
impl NoOwnerProof {
    pub(crate) fn sealed() -> Self {
        Self { _sealed: () }
    }
}
#[derive(Debug)]
pub enum OwnershipDecision {
    Acquired,
    Occupied(NoOwnerProof),
}
pub struct OfflinePreparation {
    pub(crate) scope: OfflineScope,
    verifier: PermissionVerifier,
    transferred: bool,
}
impl std::fmt::Debug for OfflinePreparation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OfflinePreparation([original resources])")
    }
}
impl OfflinePreparation {
    /// Retain the original artifact and cleanup allocation before native effects.
    pub fn new(helper: VerifiedHelper) -> Result<Self, HelperError> {
        let scope = OfflineScope::new(helper.clone(), true)?;
        let verifier = PermissionVerifier::scoped(helper, scope.clone());
        Ok(Self {
            scope,
            verifier,
            transferred: false,
        })
    }
    /// An owned view of this SAME ledger, independent of movable phase handles.
    pub fn original_cleanup_view(&self) -> OriginalCleanupView {
        OriginalCleanupView::new(self.scope.clone())
    }
    /// Separate registered verifier admission. NEVER call under NativeAdmission
    /// or Files/slot locks; Windows helper and Unix native paths reenter scope.
    pub fn permission_verifier(&self) -> &PermissionVerifier {
        &self.verifier
    }
    pub fn retain_original_directories(
        &mut self,
        files: Vec<std::fs::File>,
    ) -> Result<(), HelperError> {
        self.scope.retain_original_directories(files)
    }
    /// Stores an OWNED clone of root's same Arc-backed original allocation.
    /// Does not store a Rust borrow, reopen a path or take the caller's File.
    pub fn bind_root_borrowed(&mut self, root: &BoundOwnershipRoot) -> Result<(), HelperError> {
        self.scope.bind(root)
    }
    pub fn acquired_view(&self) -> Result<AcquiredOwnershipView<'_>, HelperError> {
        let (_, control) = self.scope.acquired()?;
        Ok(AcquiredOwnershipView::new(self, control))
    }
    pub async fn acquire_borrowed(&mut self) -> Result<OwnershipDecision, HelperError> {
        let root = self.scope.begin_acquire()?;
        let mut command = OfflineCommand::spawn_in(
            &self.scope,
            &[
                Path::new("--ownership"),
                root.path(),
                Path::new(root.identity().argument()),
            ],
            false,
        )?;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Locked {
            locked: bool,
        }
        let result = tokio::time::timeout(Duration::from_secs(8), async {
            let locked: Locked = serde_json::from_slice(&command.line().await?)
                .map_err(|_| HelperError::Protocol)?;
            if !locked.locked {
                command.expect_exit().await?;
                return Ok(false);
            }
            command.hold_ownership()?;
            Ok(true)
        })
        .await
        .map_err(|_| HelperError::Deadline)
        .and_then(|value| value);
        match result {
            Ok(true) => {
                self.scope.mark_owned(&command.control)?;
                Ok(OwnershipDecision::Acquired)
            }
            Ok(false) => Ok(OwnershipDecision::Occupied(
                self.shutdown_confirmed().await?,
            )),
            Err(error) => {
                command.terminate().await?;
                Err(error)
            }
        }
    }
    /// Compatibility typestate move. Native keepers use the borrowed operations.
    pub fn bind_root(
        mut self,
        root: BoundOwnershipRoot,
    ) -> Result<OwnershipAttempt, BindRootFailure> {
        if let Err(error) = self.bind_root_borrowed(&root) {
            return Err(BindRootFailure {
                error,
                preparation: Box::new(self),
            });
        }
        Ok(OwnershipAttempt { preparation: self })
    }
    pub async fn shutdown_confirmed(&mut self) -> Result<NoOwnerProof, HelperError> {
        self.scope.cleanup_confirmed().await
    }
}
impl Drop for OfflinePreparation {
    fn drop(&mut self) {
        if !self.transferred {
            self.scope.close();
        }
    }
}

pub struct BindRootFailure {
    pub error: HelperError,
    pub preparation: Box<OfflinePreparation>,
}
impl std::fmt::Debug for BindRootFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BindRootFailure")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Display for BindRootFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}
impl std::error::Error for BindRootFailure {}
impl BindRootFailure {
    pub fn into_parts(self) -> (HelperError, OfflinePreparation) {
        (self.error, *self.preparation)
    }
}
pub struct OwnershipAttempt {
    preparation: OfflinePreparation,
}
impl std::fmt::Debug for OwnershipAttempt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OwnershipAttempt([original resources])")
    }
}
pub struct AcquireTransferFailure {
    pub error: HelperError,
    pub attempt: Box<OwnershipAttempt>,
}
impl std::fmt::Debug for AcquireTransferFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcquireTransferFailure")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Display for AcquireTransferFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}
impl std::error::Error for AcquireTransferFailure {}
impl AcquireTransferFailure {
    pub fn into_parts(self) -> (HelperError, OwnershipAttempt) {
        (self.error, *self.attempt)
    }
}
impl OwnershipAttempt {
    pub fn root_lifetime(&self) -> crate::RootLifetimeGuard {
        self.preparation.scope.root_lifetime()
    }
    pub fn original_cleanup_view(&self) -> OriginalCleanupView {
        self.preparation.original_cleanup_view()
    }
    pub fn permission_verifier(&self) -> &PermissionVerifier {
        self.preparation.permission_verifier()
    }
    pub fn acquired_view(&self) -> Result<AcquiredOwnershipView<'_>, HelperError> {
        self.preparation.acquired_view()
    }
    pub async fn acquire(&mut self) -> Result<OwnershipDecision, HelperError> {
        self.preparation.acquire_borrowed().await
    }
    /// Compatibility transfers only facades; original control stays in the ledger.
    pub fn into_acquired(mut self) -> Result<HostOwnership, AcquireTransferFailure> {
        let (root, owner) = match self.preparation.scope.recorded_acquired() {
            Ok(acquired) => acquired,
            Err(_) => {
                return Err(AcquireTransferFailure {
                    error: HelperError::Stopped,
                    attempt: Box::new(self),
                })
            }
        };
        self.preparation.transferred = true;
        Ok(HostOwnership::from_preparation(
            self.preparation,
            root,
            owner,
        ))
    }
    pub async fn shutdown_confirmed(&mut self) -> Result<NoOwnerProof, HelperError> {
        self.preparation.shutdown_confirmed().await
    }
}
