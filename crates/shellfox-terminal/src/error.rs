use crate::PtySession;
use std::sync::Mutex;

/// A failed spawn can still own resources if cleanup could not be confirmed.
#[derive(Debug)]
pub struct SpawnError {
    pub(crate) error: anyhow::Error,
    pub(crate) session: Mutex<Option<Box<PtySession>>>,
}
impl SpawnError {
    pub fn take_session(&self) -> Option<PtySession> {
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .map(|session| *session)
    }
}
impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}
impl std::error::Error for SpawnError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.error.as_ref())
    }
}
impl From<anyhow::Error> for SpawnError {
    fn from(error: anyhow::Error) -> Self {
        Self {
            error,
            session: Mutex::new(None),
        }
    }
}

/// The shell isn't reading its input as fast as it arrives. This input was not
/// written; the terminal stays open and accepts input again once the shell reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputBusy;
impl std::fmt::Display for InputBusy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("The terminal isn't reading its input yet; this input was not written.")
    }
}
impl std::error::Error for InputBusy {}
