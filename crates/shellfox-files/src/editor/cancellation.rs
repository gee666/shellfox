use crate::{ErrorCode, FileError, Result};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone, Default)]
pub struct EditorCancellation(Arc<AtomicBool>);
impl EditorCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
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
