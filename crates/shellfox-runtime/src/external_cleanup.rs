//! Retained parser-thread ownership for external startup and cancellation.
use crate::*;
use std::{io, thread::JoinHandle};
#[path = "external_cleanup_registry.rs"]
mod registry;
pub use registry::{poll_external_cleanup, ExternalCleanupReport};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalStartStage {
    BeforeThread,
    OwnerUnconfirmed,
}
pub struct ExternalStartError {
    pub stage: ExternalStartStage,
    error: io::Error,
    owner: Mutex<Option<Box<ExternalStartupOwner>>>,
}
impl ExternalStartError {
    pub fn take_owner(&self) -> Option<ExternalStartupOwner> {
        self.owner
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
            .map(|owner| *owner)
    }
    pub(crate) fn before(error: io::Error) -> Self {
        Self {
            stage: ExternalStartStage::BeforeThread,
            error,
            owner: Mutex::new(None),
        }
    }
    pub(crate) fn retained(error: io::Error, mut runtime: Runtime) -> Self {
        cancel(&mut runtime);
        Self {
            stage: ExternalStartStage::OwnerUnconfirmed,
            error,
            owner: Mutex::new(Some(Box::new(ExternalStartupOwner { runtime }))),
        }
    }
}
impl std::fmt::Debug for ExternalStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExternalStartError")
            .field("stage", &self.stage)
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Display for ExternalStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "External parser startup failed ({:?}): {}",
            self.stage, self.error
        )
    }
}
impl std::error::Error for ExternalStartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}
/// No source publisher is exposed on startup failure. Keep the reserved run and
/// this controller until poll_closed confirms the original parser thread joined.
pub struct ExternalStartupOwner {
    runtime: Runtime,
}
impl ExternalStartupOwner {
    pub fn identity(&self) -> Identity {
        self.runtime.identity()
    }
    pub fn request_cancel(&mut self) {
        cancel(&mut self.runtime);
    }
    pub fn poll_closed(&mut self) -> bool {
        self.runtime
            .external_control
            .as_mut()
            .is_none_or(ExternalControl::poll_closed)
    }
}
fn cancel(runtime: &mut Runtime) {
    runtime.shared.abandon();
    runtime.external_source.take();
}
pub(crate) struct ExternalControl {
    owner: Option<JoinHandle<()>>,
    record: registry::Reservation,
}
impl ExternalControl {
    pub(crate) fn prepare(status: Arc<Mutex<ExternalStatus>>) -> io::Result<registry::Reservation> {
        registry::reserve(status)
    }
    pub(crate) fn poll_closed(&mut self) -> bool {
        if self
            .owner
            .as_ref()
            .is_some_and(|owner| !owner.is_finished())
        {
            return false;
        }
        if let Some(owner) = self.owner.take() {
            let _ = owner.join();
            self.record.joined();
        }
        true
    }
}
impl Drop for ExternalControl {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.take() {
            self.record.handoff(owner);
        }
    }
}
