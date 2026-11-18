use accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_NODES: usize = 8192;
// One full runtime screen permits 16 MiB, with room for the editor and control text.
pub const MAX_TEXT_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemanticError {
    DuplicateId(NodeId),
    MissingNode(NodeId),
    MultipleParents(NodeId),
    Cycle(NodeId),
    Unreachable(NodeId),
    InvalidBounds(NodeId),
    InvalidText(NodeId),
    InvalidFocus(NodeId),
    FrameGeneration,
    FrameWindow,
    FrameNotCollecting,
    #[cfg(test)]
    FrameNotComplete,
    FramePostPaint,
    UnpaintedOverlay(NodeId),
    InvalidIdKey,
    InvalidIdCount,
    IdKeyConflict,
    IdExhausted,
    TerminalStamp(NodeId),
    Limit,
    Native(String),
}

impl std::fmt::Display for SemanticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "accessibility: {self:?}")
    }
}
impl std::error::Error for SemanticError {}

/// A complete, validated window tree. IDs must remain stable for the lifetime of an item.
/// Never recycle an ID for a different session, tab, document or terminal generation.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticTree {
    pub(super) root: NodeId,
    pub(super) focus: NodeId,
    pub(super) nodes: BTreeMap<NodeId, Node>,
    pub(super) terminal_views: BTreeMap<NodeId, super::TerminalViewStamp>,
}

impl SemanticTree {
    pub fn new(
        root: NodeId,
        focus: NodeId,
        nodes: impl IntoIterator<Item = (NodeId, Node)>,
    ) -> Result<Self, SemanticError> {
        let mut map = BTreeMap::new();
        let mut text_bytes = 0usize;
        for (id, node) in nodes {
            if map.len() == MAX_NODES {
                return Err(SemanticError::Limit);
            }
            text_bytes = text_bytes
                .saturating_add(node.label().map_or(0, str::len))
                .saturating_add(node.value().map_or(0, str::len));
            if text_bytes > MAX_TEXT_BYTES {
                return Err(SemanticError::Limit);
            }
            if let Some(bounds) = node.bounds()
                && (![bounds.x0, bounds.y0, bounds.x1, bounds.y1]
                    .into_iter()
                    .all(f64::is_finite)
                    || bounds.x1 < bounds.x0
                    || bounds.y1 < bounds.y0)
            {
                return Err(SemanticError::InvalidBounds(id));
            }
            if node.role() == Role::TextRun {
                let lengths = node.character_lengths();
                let text = node.value().unwrap_or("");
                let mut offset = 0usize;
                for len in lengths {
                    if *len == 0 {
                        return Err(SemanticError::InvalidText(id));
                    }
                    offset += usize::from(*len);
                    if !text.is_char_boundary(offset) {
                        return Err(SemanticError::InvalidText(id));
                    }
                }
                if offset != text.len() {
                    return Err(SemanticError::InvalidText(id));
                }
                for values in [node.character_positions(), node.character_widths()]
                    .into_iter()
                    .flatten()
                {
                    if values.len() != lengths.len()
                        || values.iter().any(|value| !value.is_finite())
                    {
                        return Err(SemanticError::InvalidText(id));
                    }
                }
                if node
                    .character_widths()
                    .is_some_and(|widths| widths.iter().any(|width| *width < 0.))
                {
                    return Err(SemanticError::InvalidText(id));
                }
            }
            if map.insert(id, node).is_some() {
                return Err(SemanticError::DuplicateId(id));
            }
        }
        let tree = Self {
            root,
            focus,
            nodes: map,
            terminal_views: BTreeMap::new(),
        };
        tree.validate_links()?;
        Ok(tree)
    }

    fn validate_links(&self) -> Result<(), SemanticError> {
        let mut parents = BTreeSet::new();
        for node in self.nodes.values() {
            for child in node.children() {
                if !self.nodes.contains_key(child) {
                    return Err(SemanticError::MissingNode(*child));
                }
                if !parents.insert(*child) {
                    return Err(SemanticError::MultipleParents(*child));
                }
            }
        }
        let mut visited = BTreeSet::new();
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            if !visited.insert(id) {
                return Err(SemanticError::Cycle(id));
            }
            let node = self.nodes.get(&id).ok_or(SemanticError::MissingNode(id))?;
            stack.extend(node.children());
        }
        if let Some(id) = self.nodes.keys().find(|id| !visited.contains(id)) {
            return Err(SemanticError::Unreachable(*id));
        }
        if !self.nodes.contains_key(&self.focus)
            || self.focus != self.root && !self.is_exposed(self.focus)
        {
            return Err(SemanticError::InvalidFocus(self.focus));
        }
        for (id, node) in &self.nodes {
            if let Some(selection) = node.text_selection() {
                for position in [&selection.anchor, &selection.focus] {
                    let run = self
                        .nodes
                        .get(&position.node)
                        .ok_or(SemanticError::InvalidText(*id))?;
                    if run.role() != Role::TextRun
                        || position.character_index > run.character_lengths().len()
                        || !self.descendant(*id, position.node)
                    {
                        return Err(SemanticError::InvalidText(*id));
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn descendant(&self, parent: NodeId, child: NodeId) -> bool {
        let mut stack = vec![parent];
        while let Some(id) = stack.pop() {
            if id == child {
                return true;
            }
            if let Some(node) = self.nodes.get(&id) {
                stack.extend(node.children());
            }
        }
        false
    }

    pub(crate) fn is_exposed(&self, target: NodeId) -> bool {
        let mut stack = vec![self.root];
        while let Some(id) = stack.pop() {
            let Some(node) = self.nodes.get(&id) else {
                continue;
            };
            if node.is_hidden() || node.is_disabled() {
                continue;
            }
            if id == target {
                return true;
            }
            stack.extend(node.children());
        }
        false
    }

    pub(super) fn with_terminal_views(
        mut self,
        views: BTreeMap<NodeId, super::TerminalViewStamp>,
    ) -> Result<Self, SemanticError> {
        for (id, stamp) in &views {
            if self
                .node(*id)
                .is_none_or(|node| node.role() != Role::Terminal)
                || stamp.version == 0
                || stamp.top_row > stamp.total_rows
            {
                return Err(SemanticError::TerminalStamp(*id));
            }
        }
        self.terminal_views = views;
        Ok(self)
    }

    pub fn terminal_view(&self, id: NodeId) -> Option<super::TerminalViewStamp> {
        self.terminal_views.get(&id).copied()
    }

    pub(super) fn same_native_tree(&self, other: &Self) -> bool {
        self.root == other.root && self.focus == other.focus && self.nodes == other.nodes
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub(crate) fn full_update(&self) -> TreeUpdate {
        TreeUpdate {
            nodes: self
                .nodes
                .iter()
                .map(|(id, node)| (*id, node.clone()))
                .collect(),
            tree: Some(TreeInfo::new(self.root)),
            tree_id: TreeId::ROOT,
            focus: self.focus,
        }
    }

    pub(crate) fn delta(&self, previous: &Self) -> TreeUpdate {
        if self.root != previous.root {
            return self.full_update();
        }
        TreeUpdate {
            nodes: self
                .nodes
                .iter()
                .filter(|(id, node)| previous.nodes.get(id) != Some(node))
                .map(|(id, node)| (*id, node.clone()))
                .collect(),
            tree: None,
            tree_id: TreeId::ROOT,
            focus: self.focus,
        }
    }
}
