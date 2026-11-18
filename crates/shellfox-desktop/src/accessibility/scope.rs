use super::{SemanticError, SemanticFrame};

pub(super) struct OverlayScope {
    frame: SemanticFrame,
}

impl Drop for OverlayScope {
    fn drop(&mut self) {
        self.frame.exit_overlay();
        if std::thread::panicking() {
            self.frame.reject(SemanticError::FramePostPaint);
        }
    }
}

impl SemanticFrame {
    pub(super) fn overlay_scope(&self) -> OverlayScope {
        self.enter_overlay();
        OverlayScope {
            frame: self.clone(),
        }
    }
}
