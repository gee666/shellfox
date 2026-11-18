use super::{SemanticError, SemanticFrame};
use accesskit::NodeId;

/// Own one context in each Root. Clones retain the same UI-thread collector.
/// The owning Root attaches and owns the native provider separately.
#[derive(Clone)]
pub struct WindowSemanticContext {
    frame: SemanticFrame,
}

impl SemanticFrame {
    /// Exact logical keys share one window-lifetime registry, never a hash-derived ID.
    pub fn id_for(&self, key: &str) -> Result<NodeId, SemanticError> {
        self.ids.borrow_mut().id_for(key)
    }
    pub fn reserve_ids(&self, key: &str, count: usize) -> Result<Vec<NodeId>, SemanticError> {
        self.ids.borrow_mut().reserve_ids(key, count)
    }
}

impl WindowSemanticContext {
    pub fn new(root: NodeId) -> Self {
        Self {
            frame: SemanticFrame::new(root),
        }
    }
    pub fn frame(&self) -> SemanticFrame {
        self.frame.clone()
    }
    pub fn root_id(&self) -> NodeId {
        self.frame.root_id()
    }
}
