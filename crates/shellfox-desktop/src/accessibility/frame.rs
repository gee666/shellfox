use super::{SemanticError, SemanticTree};
mod support;
use accesskit::{Node, NodeId, Role};
use gpui::{AnyWindowHandle, FocusHandle, Window};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::{Rc, Weak},
};

#[derive(Clone)]
pub struct SemanticNode {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub data: Node,
    pub focus: Option<FocusHandle>,
}

impl SemanticNode {
    pub fn new(id: NodeId, parent: Option<NodeId>, role: Role, name: impl Into<String>) -> Self {
        let mut data = Node::new(role);
        data.set_label(name.into());
        Self {
            id,
            parent,
            data,
            focus: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Collecting,
    Complete,
}

struct FrameData {
    window: Option<AnyWindowHandle>,
    generation: u64,
    phase: Phase,
    nodes: Vec<SemanticNode>,
    handles: BTreeMap<NodeId, FocusHandle>,
    focus: Option<NodeId>,
    error: Option<SemanticError>,
    pending_error: Option<SemanticError>,
    overlay_depth: usize,
    overlays: std::collections::BTreeSet<NodeId>,
    expected_overlays: std::collections::BTreeSet<NodeId>,
    terminal_views: BTreeMap<NodeId, super::TerminalViewStamp>,
}

impl FrameData {
    fn new() -> Self {
        Self {
            window: None,
            generation: 0,
            phase: Phase::Idle,
            nodes: Vec::new(),
            handles: BTreeMap::new(),
            focus: None,
            error: None,
            pending_error: None,
            overlay_depth: 0,
            overlays: std::collections::BTreeSet::new(),
            expected_overlays: std::collections::BTreeSet::new(),
            terminal_views: BTreeMap::new(),
        }
    }

    fn tree(&self, root: NodeId) -> Result<SemanticTree, SemanticError> {
        if let Some(error) = self.error.as_ref().or(self.pending_error.as_ref()) {
            return Err(error.clone());
        }
        if let Some(id) = self
            .expected_overlays
            .iter()
            .find(|id| !self.overlays.contains(id))
        {
            return Err(SemanticError::UnpaintedOverlay(*id));
        }
        let mut nodes = Vec::with_capacity(self.nodes.len());
        let mut positions = BTreeMap::new();
        for node in &self.nodes {
            if positions.insert(node.id, nodes.len()).is_some() {
                return Err(SemanticError::DuplicateId(node.id));
            }
            nodes.push((node.id, node.data.clone()));
        }
        for node in &self.nodes {
            if let Some(parent) = node.parent {
                let index = positions
                    .get(&parent)
                    .ok_or(SemanticError::MissingNode(parent))?;
                nodes[*index].1.push_child(node.id);
            }
        }
        SemanticTree::new(root, self.focus.unwrap_or(root), nodes)?
            .with_terminal_views(self.terminal_views.clone())
    }
}

/// A token belongs to one collector and one paint, not just a numeric frame index.
pub struct FrameGeneration {
    generation: u64,
    owner: Weak<RefCell<FrameData>>,
}

/// Retain this collector for the window's lifetime and clone it into child entities.
/// Every paint measures the nodes again; `cached` replays those of a reused cached view.
/// Only begin_frame resets its contents. Root::render must not replace the collector.
#[derive(Clone)]
pub struct SemanticFrame {
    root: NodeId,
    data: Rc<RefCell<FrameData>>,
    pub(super) ids: Rc<RefCell<super::ids::SemanticIds>>,
}

impl SemanticFrame {
    pub fn new(root: NodeId) -> Self {
        Self {
            root,
            data: Rc::new(RefCell::new(FrameData::new())),
            ids: Rc::new(RefCell::new(super::ids::SemanticIds::new(root))),
        }
    }

    pub fn root_id(&self) -> NodeId {
        self.root
    }

    pub fn same_context(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.data, &other.data)
    }

    /// Call immediately before the complete root child's paint, not during render/prepaint.
    /// frame.finish does this automatically, including when its element is retained.
    pub fn begin_frame(&self) -> FrameGeneration {
        let mut data = self.data.borrow_mut();
        data.generation = data.generation.wrapping_add(1);
        data.phase = Phase::Collecting;
        data.nodes.clear();
        data.handles.clear();
        data.overlays.clear();
        data.expected_overlays.clear();
        data.terminal_views.clear();
        data.overlay_depth = 0;
        data.focus = None;
        data.error = data.pending_error.take();
        FrameGeneration {
            generation: data.generation,
            owner: Rc::downgrade(&self.data),
        }
    }

    /// Call only after all child painting. Stale, duplicate and cross-window finishes fail.
    pub fn finish_frame(&self, generation: FrameGeneration) -> Result<SemanticTree, SemanticError> {
        if !generation.owner.ptr_eq(&Rc::downgrade(&self.data)) {
            return Err(SemanticError::FrameGeneration);
        }
        let mut data = self.data.borrow_mut();
        if data.phase != Phase::Collecting || generation.generation != data.generation {
            return Err(SemanticError::FrameGeneration);
        }
        data.phase = Phase::Complete;
        let result = data.tree(self.root);
        data.pending_error = None;
        result
    }

    /// Call from the canvas paint callback, with its measured document/run bounds.
    pub fn insert_measured(&self, node: SemanticNode, window: &Window) {
        {
            let mut data = self.data.borrow_mut();
            if data
                .window
                .is_some_and(|owner| owner != window.window_handle())
            {
                data.error = Some(SemanticError::FrameWindow);
                return;
            }
            data.window = Some(window.window_handle());
        }
        let focused = node
            .focus
            .as_ref()
            .is_some_and(|handle| handle.is_focused(window));
        self.record(node, focused);
    }

    pub(super) fn record(&self, mut node: SemanticNode, focused: bool) {
        let mut frame = self.data.borrow_mut();
        if frame.phase != Phase::Collecting {
            frame.error = Some(SemanticError::FrameNotCollecting);
            return;
        }
        if frame.nodes.len() >= super::tree::MAX_NODES {
            frame.error = Some(SemanticError::Limit);
            return;
        }
        if let Some(handle) = &node.focus {
            node.data.add_action(accesskit::Action::Focus);
            if focused && !node.data.is_hidden() && !node.data.is_disabled() {
                frame.focus = Some(node.id);
            }
            frame.handles.insert(node.id, handle.clone());
        }
        if frame.overlay_depth > 0 {
            frame.overlays.insert(node.id);
        }
        frame.nodes.push(node);
    }

    pub(super) fn collecting_error(&self) -> Option<SemanticError> {
        self.data.borrow().error.clone()
    }

    pub(super) fn recorded(&self) -> usize {
        self.data.borrow().nodes.len()
    }

    pub(super) fn recorded_since(&self, start: usize) -> Vec<SemanticNode> {
        self.data
            .borrow()
            .nodes
            .get(start..)
            .map(<[_]>::to_vec)
            .unwrap_or_default()
    }

    /// Fail publication without changing the original UI. Render-time errors survive begin_frame.
    pub fn reject(&self, error: SemanticError) {
        self.record_error(error);
    }

    pub(super) fn record_error(&self, error: SemanticError) {
        let mut data = self.data.borrow_mut();
        data.pending_error.get_or_insert(error.clone());
        if data.phase == Phase::Collecting {
            data.error.get_or_insert(error);
        }
    }

    pub(super) fn enter_overlay(&self) {
        self.data.borrow_mut().overlay_depth += 1;
    }
    pub(super) fn exit_overlay(&self) {
        self.data.borrow_mut().overlay_depth -= 1;
    }

    pub(super) fn expect_overlay(&self, id: NodeId) {
        self.data.borrow_mut().expected_overlays.insert(id);
    }

    /// Post-effect completion runs after normal, deferred, prompt, drag and tooltip painting.
    pub(super) fn complete_after_draw(
        &self,
        generation: FrameGeneration,
    ) -> Option<Result<SemanticTree, SemanticError>> {
        if !self.is_current(&generation) {
            return None;
        }
        Some(self.finish_frame(generation))
    }

    pub fn focus_handle(&self, id: NodeId) -> Option<FocusHandle> {
        self.data.borrow().handles.get(&id).cloned()
    }

    /// Test inspection only. Production publishes the result of finish_frame after drawing.
    #[cfg(test)]
    pub fn tree(&self) -> Result<SemanticTree, SemanticError> {
        let data = self.data.borrow();
        if data.phase != Phase::Complete {
            return Err(SemanticError::FrameNotComplete);
        }
        data.tree(self.root)
    }
}
