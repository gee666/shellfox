//! Transport restrictions only. No Files capability, read grant or writer API.
use crate::{
    offline_control::OfflineControl,
    offline_scope::{Ledger, OfflineScope},
    HelperError, NoOwnerProof, OfflinePreparation, PermissionVerifier, RootLifetimeGuard,
};
use std::sync::{Arc, MutexGuard};
use tokio_util::sync::CancellationToken;

/// Retained cleanup access to the SAME original ledger through phase moves.
/// Drop does not close admission. This is not Service or artifact cleanup proof.
pub struct OriginalCleanupView {
    scope: OfflineScope,
}
impl OriginalCleanupView {
    pub(crate) fn new(scope: OfflineScope) -> Self {
        Self { scope }
    }
    /// Synchronous drain of scope-held native operations. Caller must hold no
    /// NativeAdmission, Files MUTATIONS, publication slot or verifier operation.
    pub fn seal(&self) {
        self.scope.seal();
    }
    /// Seals and waits original children. Error/cancel retains the same evidence.
    pub async fn shutdown_confirmed(&self) -> Result<NoOwnerProof, HelperError> {
        self.scope.cleanup_confirmed().await
    }
}
impl std::fmt::Debug for OriginalCleanupView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OriginalCleanupView([same original scope])")
    }
}

/// Borrowed acquired phase, never a transferable cleanup owner.
pub struct AcquiredOwnershipView<'a> {
    preparation: &'a OfflinePreparation,
    control: Arc<OfflineControl>,
}
impl<'a> AcquiredOwnershipView<'a> {
    pub(crate) fn new(preparation: &'a OfflinePreparation, control: Arc<OfflineControl>) -> Self {
        Self {
            preparation,
            control,
        }
    }
    /// Separate registered admission. NEVER invoke under NativeAdmission or
    /// Files/slot locks: the scoped verifier itself locks this same ledger.
    pub fn permission_verifier(&self) -> &PermissionVerifier {
        self.preparation.permission_verifier()
    }
    /// Directory lifetime ONLY. Cannot authorize a native operation.
    pub fn root_lifetime(&self) -> RootLifetimeGuard {
        self.preparation.scope.root_lifetime()
    }
    /// Health notification ONLY. Cancellation/EOF is not positive exit evidence.
    pub fn lost(&self) -> CancellationToken {
        self.control.lost()
    }
    /// Observation only; takes the scope lock. Under NativeAdmission use its
    /// cached check_original_owner instead, never reenter through this view.
    pub fn is_alive(&self) -> bool {
        self.preparation
            .scope
            .forward()
            .and_then(|ledger| ledger.check_owner(&self.control))
            .is_ok()
    }
    /// Owned restriction view, retaining the SAME scope and original controller.
    pub fn native_fence(&self) -> NativeOwnershipFence {
        NativeOwnershipFence {
            scope: self.preparation.scope.clone(),
            control: self.control.clone(),
        }
    }
}

/// Immutable restriction of existing native authority. Never itself a capability.
pub struct NativeOwnershipFence {
    scope: OfflineScope,
    control: Arc<OfflineControl>,
}
impl NativeOwnershipFence {
    /// Enter BEFORE any Files MUTATIONS/slot. No await or scoped verifier may
    /// occur while this guard is live. Seal waits for it, including native I/O.
    pub fn enter(&self) -> Result<NativeAdmission<'_>, HelperError> {
        let ledger = self.scope.forward()?;
        ledger.check_owner(&self.control)?;
        Ok(NativeAdmission {
            ledger,
            control: &self.control,
        })
    }
}
impl std::fmt::Debug for NativeOwnershipFence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NativeOwnershipFence([same scope and original controller])")
    }
}

/// Holds the EXISTING scope mutex through a synchronous native segment. The
/// std MutexGuard makes this !Send. Drop unlocks only, never certifies cleanup.
pub struct NativeAdmission<'a> {
    ledger: MutexGuard<'a, Ledger>,
    control: &'a Arc<OfflineControl>,
}
impl NativeAdmission<'_> {
    /// Cached leaf restriction: no poll_exit/try_wait, release, pipe changes,
    /// native I/O, callbacks, or reentry into scope/Files/slot admission.
    pub fn check_original_owner(&self) -> Result<(), HelperError> {
        self.ledger.check_owner(self.control)
    }
}
