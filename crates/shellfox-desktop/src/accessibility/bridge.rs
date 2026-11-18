use super::{TerminalViewStamp, tree::SemanticTree};
use accesskit::{
    Action, ActionData, ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, Node,
    NodeId, TextSelection, TreeId, TreeUpdate,
};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

const MAX_ACTIONS: usize = 64;
// Electron's file editor permits an 8 MiB replacement; queue bytes have a separate cap.
const MAX_ACTION_TEXT: usize = 8 * 1024 * 1024;
const MAX_QUEUED_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub enum SemanticActionKind {
    Focus,
    Click,
    SetValue(String),
    SetNumericValue(f64),
    ReplaceSelectedText(String),
    SetTextSelection(TextSelection),
    ShowContextMenu,
    Expand,
    Collapse,
    Increment,
    Decrement,
    ScrollIntoView,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SemanticAction {
    pub target: NodeId,
    pub kind: SemanticActionKind,
    /// Admission-time terminal identity. Never filled or replaced while draining.
    pub terminal_stamp: Option<TerminalViewStamp>,
}

struct Pending {
    action: SemanticAction,
    expected: Node,
    text_runs: Vec<(NodeId, Node)>,
}

#[derive(Default)]
struct State {
    tree: Option<SemanticTree>,
    pending: VecDeque<Pending>,
    pending_bytes: usize,
    closed: bool,
}

/// OS callbacks may run on arbitrary threads. They only read snapshots and enqueue work.
#[derive(Clone, Default)]
pub(crate) struct Bridge {
    state: Arc<Mutex<State>>,
    pub(crate) needs_full: Arc<AtomicBool>,
}

impl Bridge {
    pub(crate) fn replace(&self, tree: SemanticTree) {
        if let Ok(mut state) = self.state.lock()
            && !state.closed
        {
            state.tree = Some(tree);
        }
    }

    pub(crate) fn close(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.closed = true;
            state.tree = None;
            state.pending.clear();
            state.pending_bytes = 0;
        }
    }

    pub(crate) fn drain(&self) -> Vec<SemanticAction> {
        let Ok(mut state) = self.state.lock() else {
            return Vec::new();
        };
        let pending = std::mem::take(&mut state.pending);
        state.pending_bytes = 0;
        let Some(tree) = &state.tree else {
            return Vec::new();
        };
        pending
            .into_iter()
            .filter(|pending| {
                let valid = tree.is_exposed(pending.action.target)
                    && tree.node(pending.action.target) == Some(&pending.expected)
                    && pending
                        .text_runs
                        .iter()
                        .all(|(id, node)| tree.node(*id) == Some(node));
                if !valid && pending.action.terminal_stamp.is_some() {
                    self.needs_full.store(true, Ordering::Release);
                }
                valid
            })
            .map(|pending| pending.action)
            .collect()
    }

    fn translate(tree: &SemanticTree, request: ActionRequest) -> Option<SemanticAction> {
        if request.target_tree != TreeId::ROOT {
            return None;
        }
        let node = tree.node(request.target_node)?;
        if !tree.is_exposed(request.target_node) || !node.supports_action(request.action) {
            return None;
        }
        let kind = match (request.action, request.data) {
            (Action::Focus, None) => SemanticActionKind::Focus,
            (Action::Click, None) => SemanticActionKind::Click,
            (Action::SetValue, Some(ActionData::Value(value)))
                if !node.is_read_only() && value.len() <= MAX_ACTION_TEXT =>
            {
                SemanticActionKind::SetValue(value.into())
            }
            (Action::SetValue, Some(ActionData::NumericValue(value)))
                if !node.is_read_only() && value.is_finite() =>
            {
                SemanticActionKind::SetNumericValue(value)
            }
            (Action::ReplaceSelectedText, Some(ActionData::Value(value)))
                if !node.is_read_only() && value.len() <= MAX_ACTION_TEXT =>
            {
                SemanticActionKind::ReplaceSelectedText(value.into())
            }
            (Action::SetTextSelection, Some(ActionData::SetTextSelection(selection))) => {
                for point in [&selection.anchor, &selection.focus] {
                    let run = tree.node(point.node)?;
                    if run.role() != accesskit::Role::TextRun
                        || point.character_index > run.character_lengths().len()
                        || !tree.descendant(request.target_node, point.node)
                    {
                        return None;
                    }
                }
                SemanticActionKind::SetTextSelection(selection)
            }
            (Action::ShowContextMenu, None) => SemanticActionKind::ShowContextMenu,
            (Action::Expand, None) => SemanticActionKind::Expand,
            (Action::Collapse, None) => SemanticActionKind::Collapse,
            (Action::Increment, None) => SemanticActionKind::Increment,
            (Action::Decrement, None) => SemanticActionKind::Decrement,
            (Action::ScrollIntoView, None | Some(ActionData::ScrollHint(_))) => {
                SemanticActionKind::ScrollIntoView
            }
            _ => return None,
        };
        Some(SemanticAction {
            target: request.target_node,
            terminal_stamp: tree.terminal_view(request.target_node),
            kind,
        })
    }
}

impl ActivationHandler for Bridge {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.needs_full.store(true, Ordering::Release);
        self.state
            .lock()
            .ok()?
            .tree
            .as_ref()
            .map(SemanticTree::full_update)
    }
}

impl DeactivationHandler for Bridge {
    fn deactivate_accessibility(&mut self) {
        self.needs_full.store(true, Ordering::Release);
    }
}

impl ActionHandler for Bridge {
    fn do_action(&mut self, request: ActionRequest) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.closed || state.pending.len() >= MAX_ACTIONS {
            return;
        }
        let Some(tree) = &state.tree else {
            return;
        };
        if request.action == Action::SetTextSelection
            && tree
                .node(request.target_node)
                .is_some_and(|node| node.role() == accesskit::Role::Terminal)
            && tree.terminal_view(request.target_node).is_none()
        {
            self.needs_full.store(true, Ordering::Release);
            return;
        }
        let Some(action) = Self::translate(tree, request) else {
            return;
        };
        let Some(expected) = tree.node(action.target) else {
            return;
        };
        let text_bytes =
            |node: &Node| node.value().map_or(0, str::len) + node.label().map_or(0, str::len);
        let mut bytes = text_bytes(expected);
        if let SemanticActionKind::SetValue(text) | SemanticActionKind::ReplaceSelectedText(text) =
            &action.kind
        {
            bytes += text.len();
        }
        let mut endpoints = Vec::new();
        if let SemanticActionKind::SetTextSelection(selection) = &action.kind {
            for id in [selection.anchor.node, selection.focus.node] {
                if let Some(node) = tree.node(id) {
                    bytes += text_bytes(node);
                    endpoints.push((id, node));
                }
            }
        }
        if bytes > MAX_QUEUED_BYTES.saturating_sub(state.pending_bytes) {
            return;
        }
        let expected = expected.clone();
        let text_runs = endpoints
            .into_iter()
            .map(|(id, node)| (id, node.clone()))
            .collect();
        state.pending_bytes += bytes;
        state.pending.push_back(Pending {
            action,
            expected,
            text_runs,
        });
    }
}
