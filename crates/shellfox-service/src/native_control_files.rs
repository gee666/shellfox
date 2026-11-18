//! Native-only bootstrap. Issuer is never a renderer/peer or path-based grant.
use crate::security::{NativeCreationRetention, PrivateDirectory};
use anyhow::Result;
use shellfox_files::{Access, FileError, FileService, NativeChildIssuer, NativeCommitFence};
use shellfox_protocol::CancellationToken;
use shellfox_transport::{AcquiredOwnershipView, RootLifetimeGuard};
use std::sync::Arc;

struct OwnershipFence {
    lost: CancellationToken,
    _root_lifetime: RootLifetimeGuard,
    _creation_custody: Arc<NativeCreationRetention>,
}
impl NativeCommitFence for OwnershipFence {
    fn check(&self) -> shellfox_files::Result<()> {
        if self.lost.is_cancelled() {
            return Err(FileError {
                code: shellfox_files::ErrorCode::AccessDenied,
                message: "Native file ownership lost.".into(),
            });
        }
        Ok(())
    }
}
/// Trusted persistence only. No parent, path, account issuer or wire authority.
/// Mutation checks do not certify current read ownership or native cleanup.
#[derive(Clone)]
pub(crate) struct NativeControlChild {
    files: Arc<FileService>,
}
impl NativeControlChild {
    pub(crate) fn files(&self) -> Arc<FileService> {
        self.files.clone()
    }
}
pub(super) struct NativeControlFiles {
    issuer: Arc<FileService>,
    fence: Arc<dyn NativeCommitFence>,
    parent: Option<NativeChildIssuer>,
    control: Option<NativeControlChild>,
}
impl NativeControlFiles {
    /// Infallible construction AFTER acquisition. No binding or filesystem IO
    /// happens before the caller stores this beside the original Go owner.
    pub fn new(
        ownership: &AcquiredOwnershipView<'_>,
        creation: Arc<NativeCreationRetention>,
    ) -> Self {
        Self {
            issuer: Arc::new(FileService::local_desktop(Access::ReadWrite)),
            fence: Arc::new(OwnershipFence {
                lost: ownership.lost(),
                _root_lifetime: ownership.root_lifetime(),
                _creation_custody: creation,
            }),
            parent: None,
            control: None,
        }
    }
    pub fn bind(&mut self, private: &PrivateDirectory) -> Result<NativeControlChild> {
        self.fence.check()?;
        if self.control.is_some() {
            return self.child();
        }
        if self.parent.is_none() {
            // Store each successful narrowing before any next fallible step.
            self.parent = Some(private.bind_native_parent(&self.issuer, self.fence.clone())?);
        }
        let parent = self
            .parent
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Native parent binding missing"))?;
        let files = private.bind_native_control(&self.issuer, parent, self.fence.clone())?;
        self.control = Some(NativeControlChild { files });
        self.child()
    }
    pub fn child(&self) -> Result<NativeControlChild> {
        self.control
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Native control child is not prepared"))
    }
    pub fn revoke(&self) -> Result<()> {
        // Additional owned-lifecycle barrier. Synchronous fence checks remain
        // authoritative even before this barrier runs; no untracked watcher.
        if let Some(control) = &self.control {
            control.files().revoke()?;
        }
        Ok(self.issuer.revoke()?)
    }
}
