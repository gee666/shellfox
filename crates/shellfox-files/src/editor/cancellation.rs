use crate::{ErrorCode, FileError, Result};
use tokio_util::sync::CancellationToken;

/// Clones share one cancellation. Async callers can await it to abort a
/// request in flight; sync callers check it between steps.
#[derive(Clone, Default)]
pub struct EditorCancellation(CancellationToken);
impl EditorCancellation {
    pub fn cancel(&self) {
        self.0.cancel();
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
    /// Resolves once cancel() is called, at once if it already was.
    pub async fn cancelled(&self) {
        self.0.cancelled().await;
    }
    pub(super) fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(FileError::new(
                ErrorCode::AccessDenied,
                "File operation cancelled.",
            ))
        } else {
            Ok(())
        }
    }
}
