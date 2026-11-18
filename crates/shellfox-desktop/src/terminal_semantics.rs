//! Window-local terminal collection. Never owns a parser, controller or input queue.
use crate::{
    accessibility::{
        Role, SemanticAction, SemanticActionKind, SemanticError, SemanticFrame, SemanticId,
        SemanticNode, TerminalDocument, TerminalGeometry, TerminalSnapshot,
    },
    app::TerminalView,
};
use gpui::{Bounds, Context, FocusHandle, Pixels, Window};
use shellfox_runtime::{Identity, PresentationAction, PresentationLease, ViewFrame};
use std::time::Instant;

fn register_sample(
    frame: &SemanticFrame,
    id: SemanticId,
    snapshot: &TerminalSnapshot,
) -> Result<(), SemanticError> {
    let stamp = snapshot
        .terminal_stamp
        .ok_or(SemanticError::TerminalStamp(id))?;
    frame.set_terminal_view(id, stamp)
}

struct Document {
    source: (Identity, PresentationLease),
    id: SemanticId,
    text: TerminalDocument,
}
pub(crate) struct TerminalSemantics {
    frame: SemanticFrame,
    parent: SemanticId,
    key: String,
    document: Option<Document>,
}
impl TerminalSemantics {
    fn new(frame: SemanticFrame, parent: SemanticId, key: String) -> Self {
        Self {
            frame,
            parent,
            key,
            document: None,
        }
    }
    fn document(&mut self, view: &ViewFrame) -> Result<&mut Document, SemanticError> {
        let source = (view.identity, view.view);
        if self
            .document
            .as_ref()
            .is_none_or(|document| document.source != source)
        {
            let key = format!("{}/source/{:?}/{:?}", self.key, view.identity, view.view);
            let id = self.frame.id_for(&key)?;
            let rows = self.frame.reserve_ids(&format!("{key}/rows"), 250)?;
            self.document = Some(Document {
                source,
                id,
                text: TerminalDocument::new(id, rows)?,
            });
        }
        self.document
            .as_mut()
            .ok_or(SemanticError::FrameNotCollecting)
    }
    fn paint(
        &mut self,
        view: Option<&ViewFrame>,
        geometry: TerminalGeometry,
        focus: &FocusHandle,
        window: &Window,
    ) -> Result<(), SemanticError> {
        let frame = self.frame.clone();
        let parent = self.parent;
        let Some(view) = view else {
            // Fixture/loading surfaces do not provide the live metadata contract.
            let id = frame.id_for(&format!("{}/unavailable", self.key))?;
            let mut node = SemanticNode::new(
                id,
                Some(parent),
                Role::Status,
                "Terminal presentation metadata unavailable",
            );
            node.data.set_bounds(geometry.bounds);
            node.data.set_read_only();
            frame.insert_measured(node, window);
            return Ok(());
        };
        let document = self.document(view)?;
        let read_only = matches!(view.view, PresentationLease::Observer(_));
        let snapshot = document
            .text
            .snapshot_view(view, geometry, read_only, Instant::now())?;
        // Register the stamp belonging to these exact sampled runs, including coalesced samples.
        register_sample(&frame, document.id, &snapshot)?;
        frame.insert_measured(
            SemanticNode {
                id: document.id,
                parent: Some(parent),
                data: snapshot.document,
                focus: Some(focus.clone()),
            },
            window,
        );
        for (id, data) in snapshot.runs {
            frame.insert_measured(
                SemanticNode {
                    id,
                    parent: Some(document.id),
                    data,
                    focus: None,
                },
                window,
            );
        }
        Ok(())
    }
    fn selection_action(
        &self,
        action: &SemanticAction,
        current: &ViewFrame,
    ) -> Option<(PresentationLease, PresentationAction)> {
        let document = self.document.as_ref()?;
        if action.target != document.id {
            return None;
        }
        let SemanticActionKind::SetTextSelection(range) = &action.kind else {
            return None;
        };
        let captured = action.terminal_stamp?;
        if document.source != (captured.identity, captured.view) {
            return None;
        }
        let guarded =
            document
                .text
                .selection_action_for_view(range, action.terminal_stamp, current)?;
        if !matches!(&guarded, PresentationAction::SelectionAtVersion { version, .. } if *version == captured.version)
        {
            return None;
        }
        Some((captured.view, guarded))
    }
    fn refresh_due(&self, now: Instant) -> bool {
        self.document
            .as_ref()
            .is_some_and(|document| document.text.refresh_due(now))
    }
}
impl TerminalView {
    pub(crate) fn handle_terminal_semantic_action(
        &mut self,
        action: &SemanticAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self
            .semantic_binding
            .as_ref()
            .and_then(|binding| binding.document.as_ref())
            .is_some_and(|document| document.id == action.target)
        {
            return false;
        }
        match &action.kind {
            SemanticActionKind::Focus => {
                self.focus.focus(window);
                cx.notify();
            }
            SemanticActionKind::SetTextSelection(_) => {
                let mapped = self.presentation_frame().and_then(|current| {
                    self.semantic_binding
                        .as_ref()?
                        .selection_action(action, &current)
                });
                let accepted = mapped.is_some_and(|(view, guarded)| {
                    self.apply_semantic_selection(view, guarded, cx).is_ok()
                });
                if !accepted {
                    // Drop the intent. Re-sample on the next paint, never adopt a newer action stamp.
                    if let Some(binding) = &mut self.semantic_binding {
                        binding.document = None;
                    }
                    cx.notify();
                }
            }
            _ => return false,
        }
        true
    }
    pub(crate) fn set_semantic_frame(
        &mut self,
        frame: SemanticFrame,
        parent: SemanticId,
        key: String,
        cx: &mut Context<Self>,
    ) -> Result<(), SemanticError> {
        if let Some(binding) = &self.semantic_binding {
            if !binding.frame.same_context(&frame) || binding.parent != parent || binding.key != key
            {
                return Err(SemanticError::FrameWindow);
            }
        } else {
            self.semantic_binding = Some(TerminalSemantics::new(frame, parent, key));
            cx.notify();
        }
        Ok(())
    }
    pub(crate) fn paint_semantics(
        &mut self,
        view: Option<&ViewFrame>,
        bounds: Bounds<Pixels>,
        window: &Window,
    ) {
        let geometry =
            TerminalGeometry::from_layout(bounds, self.cell_width, self.line_height, window);
        if let Some(binding) = &mut self.semantic_binding
            && let Err(error) = binding.paint(view, geometry, &self.focus, window)
        {
            binding.frame.reject(error);
        }
    }
    pub(crate) fn refresh_semantics(&self, now: Instant, cx: &mut Context<Self>) {
        if self
            .semantic_binding
            .as_ref()
            .is_some_and(|binding| binding.refresh_due(now))
        {
            cx.notify();
        }
    }
}

#[cfg(test)]
#[path = "terminal_semantics_tests.rs"]
mod tests;
