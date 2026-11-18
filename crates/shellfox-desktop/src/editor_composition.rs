//! Local document authority plus a separately scoped V2 metadata root.
use crate::frontend_composition::FrontendDirectories;
use shellfox_files::EditorBackend;
use std::sync::Arc;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EditorUnavailableReason {
    PreparationFailed(String),
}
impl EditorUnavailableReason {
    pub fn message(&self) -> &str {
        match self {
            Self::PreparationFailed(reason) => reason,
        }
    }
}
#[derive(Clone)]
pub(crate) enum EditorOutcome {
    Ready(Arc<EditorBackend>),
    Unavailable(EditorUnavailableReason),
}
impl std::fmt::Debug for EditorOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready(_) => f.write_str("Ready(V2 editor)"),
            Self::Unavailable(reason) => f.debug_tuple("Unavailable").field(reason).finish(),
        }
    }
}
impl PartialEq for EditorOutcome {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Ready(a), Self::Ready(b)) => Arc::ptr_eq(a, b),
            (Self::Unavailable(a), Self::Unavailable(b)) => a == b,
            _ => false,
        }
    }
}
impl Eq for EditorOutcome {}
impl gpui::Global for EditorOutcome {}
pub(crate) fn prepare(frontend: &mut FrontendDirectories) -> EditorOutcome {
    match frontend.prepare_editor() {
        Ok(editor) => EditorOutcome::Ready(editor),
        Err(error) => EditorOutcome::Unavailable(EditorUnavailableReason::PreparationFailed(
            format!("Files unavailable: {error:#}"),
        )),
    }
}
#[cfg(test)]
#[path = "editor_composition_tests.rs"]
mod tests;
