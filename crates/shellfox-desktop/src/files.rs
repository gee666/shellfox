use crate::{editor_composition::EditorOutcome, field::Field, root::Root};
use gpui::{Context, Entity};
use shellfox_files::{EditorCancellation, FileList};
use std::sync::Arc;
#[path = "document_backend.rs"]
mod backend;
pub(crate) use backend::DocumentBackend;
#[path = "files_editor.rs"]
mod editor_ops;
#[path = "files_editor_state.rs"]
mod editor_state;
#[path = "files_backend.rs"]
mod selection_backend;
fn files_unavailable(outcome: Option<&EditorOutcome>) -> &str {
    match outcome {
        Some(EditorOutcome::Unavailable(reason)) => reason.message(),
        Some(EditorOutcome::Ready(_)) => "Files backend is unavailable.",
        None => "Files unavailable: editor admission status is missing.",
    }
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CloseGuard {
    Ready,
    Dirty,
    Pending,
}
fn close_guard(dirty: bool, busy: bool) -> CloseGuard {
    if busy {
        CloseGuard::Pending
    } else if dirty {
        CloseGuard::Dirty
    } else {
        CloseGuard::Ready
    }
}
// UI epochs are cancellation only. A peer backend independently holds the ORIGINAL admission.
fn same_ui_source(captured: u64, current: u64, backend_local: bool, selected_local: bool) -> bool {
    captured == current && backend_local == selected_local
}
pub struct Files {
    pub(crate) backend: DocumentBackend,
    pub(crate) source_epoch: u64,
    pub operation: Option<EditorCancellation>,
    pub location: Entity<Field>,
    pub listing: Option<FileList>,
    pub editor: Option<Editor>,
    pub visible: bool,
    pub browser: bool,
    pub browser_before_maximize: bool,
    pub maximized: bool,
    pub width: f32,
    pub browser_height: f32,
    pub resizing_width: bool,
    pub resizing_browser: bool,
    pub busy: bool,
    pub request: u64,
    pub pending_open: Option<String>,
}
pub struct EditorFile {
    pub path: String,
    pub size: u64,
    pub revision: String,
}
pub struct Editor {
    pub file: EditorFile,
    pub text: Entity<Field>,
    pub saved_text: Arc<str>,
    pub reload_required: bool,
}
impl Editor {
    pub fn dirty(&self, cx: &gpui::App) -> bool {
        !self.text.read(cx).matches_snapshot(&self.saved_text)
    }
}
impl Root {
    pub(crate) fn file_close_guard(&self, cx: &gpui::App) -> CloseGuard {
        close_guard(
            self.files_dirty(cx),
            self.files.as_ref().is_some_and(|f| f.busy),
        )
    }
    pub(crate) fn files_source_current(&self) -> bool {
        self.files.as_ref().is_some_and(|f| {
            same_ui_source(
                f.source_epoch,
                self.source_selection_epoch(),
                f.backend.is_local(),
                self.is_local_source(),
            ) && f.backend.check_current().is_ok()
        })
    }
    pub(crate) fn files_result_current(&self, epoch: u64, backend: &DocumentBackend) -> bool {
        same_ui_source(
            epoch,
            self.source_selection_epoch(),
            backend.is_local(),
            self.is_local_source(),
        ) && self
            .files
            .as_ref()
            .is_some_and(|f| f.source_epoch == epoch && f.backend.same_origin(backend))
            && backend.check_current().is_ok()
    }
    pub(crate) fn require_files_source(&mut self, cx: &mut Context<Self>) -> bool {
        if self.files_source_current() {
            true
        } else {
            self.error = Some(
                "Files belong to another or retired source. Return to it or reopen explicitly."
                    .into(),
            );
            cx.notify();
            false
        }
    }
    pub(crate) fn toggle_files(&mut self, cx: &mut Context<Self>) {
        if self.files_source_current() {
            if let Some(files) = &mut self.files {
                files.visible = !files.visible;
            }
            cx.notify();
            return;
        }
        if self.files.as_ref().is_some_and(|f| f.busy) {
            if let Some(files) = &mut self.files {
                files.visible = true;
            }
            self.cancel_file_operation(cx);
            self.error = Some(
                "Wait for the original file operation to finish before changing sources.".into(),
            );
            cx.notify();
            return;
        }
        if self.files_dirty(cx) {
            if let Some(files) = &mut self.files {
                files.visible = true;
            }
            self.error=Some("The original draft is retained. Explicitly discard it before opening Files for another source.".into());
            cx.notify();
            return;
        }
        let Some((backend, preferred)) = self.selected_document_backend(cx) else {
            return;
        };
        let epoch = self.source_selection_epoch();
        self.files = Some(Files {
            backend: backend.clone(),
            source_epoch: epoch,
            operation: None,
            location: Field::file_path(String::new(), cx),
            listing: None,
            editor: None,
            visible: true,
            browser: true,
            browser_before_maximize: true,
            maximized: false,
            width: 520.,
            browser_height: 240.,
            resizing_width: false,
            resizing_browser: false,
            busy: true,
            request: 0,
            pending_open: None,
        });
        let task_backend = backend.clone();
        let task = cx
            .background_executor()
            .spawn(async move { task_backend.initial_path(preferred).await });
        cx.spawn(async move |root, cx| {
            let result = task.await;
            let _ = root.update(cx, |root, cx| {
                if !root.files_result_current(epoch, &backend) {
                    if let Some(f) = &mut root.files
                        && f.source_epoch == epoch
                        && f.backend.same_origin(&backend)
                    {
                        f.busy = false;
                    }
                    cx.notify();
                    return;
                }
                if let Some(f) = &mut root.files {
                    f.busy = false;
                }
                match result {
                    Ok(path) => root.list_files(path, cx),
                    Err(error) => {
                        root.error = Some(format!("Files source unavailable: {error:#}"));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        cx.notify();
    }
    pub(crate) fn discard_retired_files(&mut self, cx: &mut Context<Self>) {
        if self.files_source_current() {
            return;
        }
        if self.files.as_ref().is_some_and(|f| f.busy) {
            self.cancel_file_operation(cx);
            self.error = Some(
                "Wait for the original operation to complete before discarding its draft.".into(),
            );
            cx.notify();
            return;
        }
        // Explicit user discard, never source relabelling or an automatic save/replay.
        self.files = None;
        self.toggle_files(cx);
    }
    pub(crate) fn files_dirty(&self, cx: &gpui::App) -> bool {
        self.files
            .as_ref()
            .and_then(|f| f.editor.as_ref())
            .is_some_and(|e| e.dirty(cx))
    }
    pub(crate) fn list_files(&mut self, path: String, cx: &mut Context<Self>) {
        if !self.require_files_source(cx) {
            return;
        }
        let Some(files) = &mut self.files else { return };
        if files.busy {
            return;
        }
        files.request += 1;
        let request = files.request;
        files.busy = true;
        let epoch = files.source_epoch;
        let backend = files.backend.clone();
        let task_backend = backend.clone();
        let task = cx
            .background_executor()
            .spawn(async move { task_backend.list(&path).await });
        cx.spawn(async move |root, cx| {
            let result = task.await;
            let _ = root.update(cx, |root, cx| {
                let current = root.files_result_current(epoch, &backend);
                let Some(files) = &mut root.files else { return };
                if files.request != request || !files.backend.same_origin(&backend) {
                    return;
                }
                files.busy = false;
                if !current {
                    root.error = Some("Folder result belongs to a retired source.".into());
                    cx.notify();
                    return;
                }
                match result {
                    Ok(listing) => {
                        let mut prepared = Some(listing);
                        let mut location = Field::file_path(
                            prepared
                                .as_ref()
                                .map(|l| l.path.clone())
                                .unwrap_or_default(),
                            cx,
                        );
                        // Owner-thread source check above; only bounded swaps under the ORIGINAL witness.
                        match backend.commit_if_current(|| {
                            std::mem::swap(&mut files.listing, &mut prepared);
                            std::mem::swap(&mut files.location, &mut location);
                        }) {
                            Ok(()) => root.error = None,
                            Err(e) => root.error = Some(format!("Folder source retired: {e:#}")),
                        }
                        drop(prepared);
                        drop(location);
                    }
                    Err(e) => root.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn files_reports_initialization_failure_instead_of_missing_backend_defaults() {
        let outcome = EditorOutcome::Unavailable(
            crate::editor_composition::EditorUnavailableReason::PreparationFailed(
                "Files unavailable: fixture initialization refused.".into(),
            ),
        );
        assert_eq!(
            files_unavailable(Some(&outcome)),
            "Files unavailable: fixture initialization refused."
        );
        assert_ne!(files_unavailable(None), files_unavailable(Some(&outcome)));
    }
    #[test]
    fn file_edit_during_async_terminal_close_blocks_final_removal() {
        assert_eq!(close_guard(false, false), CloseGuard::Ready);
        assert_eq!(close_guard(true, false), CloseGuard::Dirty);
    }
    #[test]
    fn in_flight_write_blocks_close_even_if_buffer_matches_old_saved_text() {
        assert_eq!(close_guard(false, true), CloseGuard::Pending);
        assert_eq!(close_guard(true, true), CloseGuard::Pending);
        assert_eq!(close_guard(true, false), CloseGuard::Dirty);
    }
    #[test]
    fn ui_source_switch_roundtrip_cannot_publish_local_or_foreign_results() {
        assert!(same_ui_source(1, 1, true, true));
        assert!(!same_ui_source(1, 2, true, false));
        assert!(!same_ui_source(1, 3, true, true));
        assert!(!same_ui_source(2, 2, false, true));
        assert!(same_ui_source(2, 2, false, false));
    }
}
