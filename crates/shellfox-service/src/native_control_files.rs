//! The endpoint directory as a Files scope. Built from native preparation, never from a
//! renderer, peer or path.
use crate::security::{NativeCreationRetention, PrivateDirectory};
use anyhow::Result;
use shellfox_files::{FileError, FileService, NativeCommitFence};
use shellfox_protocol::CancellationToken;
use shellfox_transport::{AcquiredOwnershipView, RootLifetimeGuard};
use std::sync::Arc;

/// Writes stop as soon as native ownership is lost.
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
/// The mutation fence does not prove current read ownership; readers check that themselves.
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
    fence: Arc<dyn NativeCommitFence>,
    control: Option<NativeControlChild>,
}
impl NativeControlFiles {
    /// No filesystem IO; binding happens later in `bind`.
    pub fn new(
        ownership: &AcquiredOwnershipView<'_>,
        creation: Arc<NativeCreationRetention>,
    ) -> Self {
        Self {
            fence: Arc::new(OwnershipFence {
                lost: ownership.lost(),
                _root_lifetime: ownership.root_lifetime(),
                _creation_custody: creation,
            }),
            control: None,
        }
    }
    pub fn bind(&mut self, private: &PrivateDirectory) -> Result<NativeControlChild> {
        self.fence.check()?;
        if self.control.is_none() {
            let files = private.bind_native_control(self.fence.clone())?;
            self.control = Some(NativeControlChild { files });
        }
        self.child()
    }
    pub fn child(&self) -> Result<NativeControlChild> {
        self.control
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Native control child is not prepared"))
    }
    pub fn revoke(&self) -> Result<()> {
        if let Some(control) = &self.control {
            control.files().revoke()?;
        }
        Ok(())
    }
}
