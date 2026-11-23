use crate::{editor_composition::EditorOutcome, field::Field, root::Root};
use gpui::{Context, Entity};
use shellfox_files::FileList;
use std::sync::Arc;
mod backend;
pub(crate) use backend::{DocumentBackend, PeerFileScope};
pub(crate) mod completion;
mod editor_ops;
mod editor_state;
pub(crate) use editor_state::FileOperation;
pub(crate) mod list;
pub(crate) mod pending;
mod preview;
pub(crate) mod resizer;
mod selection_backend;
pub(crate) mod terminal_links;
pub(crate) mod tree;
pub(crate) use preview::Preview;
pub(crate) mod image_editor;
mod layout_prefs;
mod retained;
mod resume;
pub(crate) mod scrollbar;
pub(crate) mod text_preview;
pub(crate) mod transfer;
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
    pub(crate) source: crate::root::source::Source,
    pub(crate) operation: Option<FileOperation>,
    pub(crate) resume_conflict: bool,
    check_cached_revision: bool,
    resume_retry: Option<std::time::Instant>,
    pub(crate) transfer: Option<Arc<transfer::Transfer>>,
    pub location: Entity<Field>,
    pub(crate) completion: completion::FileCompletion,
    pub(crate) browser_focus: completion::BrowserFocus,
    pub listing: Option<FileList>,
    pub(crate) tree: tree::Tree,
    pub(crate) listing_error: Option<list::ListingError>,
    pub missing_folder: Option<String>,
    pub(crate) missing_focus: [gpui::FocusHandle; 2],
    pub editor: Option<Editor>,
    pub(crate) preview: Option<Preview>,
    pub(crate) image_editor: Option<image_editor::ImageEditor>,
    pub document_path: Option<String>,
    pub document_error: Option<String>,
    pub visible: bool,
    pub(crate) panel_focus: gpui::FocusHandle,
    pub browser: bool,
    pub browser_before_maximize: bool,
    pub wrap: bool,
    pub maximized: bool,
    pub width: f32,
    pub browser_height: f32,
    pub(crate) resize: Option<resizer::FileResize>,
    pub(crate) width_focus: gpui::FocusHandle,
    pub(crate) height_focus: gpui::FocusHandle,
    pub(crate) confirm_focus: crate::files_view::confirmation::ConfirmationFocus,
    pub busy: bool,
    pub request: u64,
    pub(crate) pending_file_action: Option<pending::FileAction>,
    pub(crate) save_and_continue: bool,
}
impl Drop for Files {
    fn drop(&mut self) {
        if let Some(operation) = &self.operation {
            operation.cancel();
        }
        if let Some(transfer) = &self.transfer {
            transfer.cancel.cancel();
        }
    }
}
impl Files {
    pub(crate) fn saving(&self) -> bool {
        matches!(self.operation, Some(FileOperation::Save(_)))
    }

    pub(crate) fn opened_path(&self) -> Option<&str> {
        self.editor
            .as_ref()
            .map(|editor| editor.file.path.as_str())
            .or_else(|| self.preview.as_ref().map(Preview::path))
    }
}
pub struct EditorFile {
    pub path: String,
    pub size: u64,
    pub revision: String,
    pub(super) utf8_bom: bool,
}
pub struct Editor {
    pub file: EditorFile,
    pub text: Entity<Field>,
    pub saved_text: Arc<str>,
    pub(crate) text_preview: Option<text_preview::TextPreview>,
    pub(crate) scrollbar: scrollbar::Scrollbar,
    pub(crate) horizontal_scrollbar: scrollbar::Scrollbar,
    pub reload_required: bool,
}
impl Editor {
    pub fn dirty(&self, cx: &gpui::App) -> bool {
        !self.text.read(cx).matches_snapshot(&self.saved_text)
    }
}
impl Root {
    pub(crate) fn file_close_guard(&self, cx: &gpui::App) -> CloseGuard {
        let panels = self.files.iter().chain(self.retained_files.iter());
        close_guard(
            panels.clone().any(|files| files.dirty(cx)),
            panels
                .into_iter()
                .any(|files| {
                    (files.busy && !matches!(files.operation, Some(FileOperation::Revalidate(_))))
                        || files.transfer.is_some()
                        || files.image_editor.as_ref().is_some_and(image_editor::ImageEditor::transforming)
                }),
        )
    }
    pub(crate) fn files_source_current(&self) -> bool {
        self.files.as_ref().is_some_and(|f| {
            same_ui_source(
                f.source_epoch,
                self.source_selection_epoch(),
                f.backend.is_local(),
                self.is_local_source(),
            ) && f.source == self.peers.selected
                && (f.backend.is_local() || self.peers.selected_peer().is_some_and(|(_, state)| {
                    state.phase == crate::root::source::Phase::Connected
                }))
                && self.active_file_tab(&f.backend) && f.backend.check_current().is_ok()
        })
    }
    fn active_file_tab(&self, backend: &DocumentBackend) -> bool {
        backend.wsl_tab().is_none_or(|tab| self.selected_tab() == Some(tab))
            && backend.peer_file_scope().is_none_or(|scope| {
                self.peers.selected_peer().is_some_and(|(_, state)| {
                    state.selected_session.is_some_and(|id| id.to_string() == scope.session.to_string())
                        && state.selected_tab.is_some_and(|id| id.to_string() == scope.tab.to_string())
                })
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
            && self.active_file_tab(backend)
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
        self.sync_file_source(cx);
        if self.files.as_ref().is_some_and(|files| {
            !files.visible && !self.active_file_tab(&files.backend)
        }) {
            self.show_selected_file_scope(cx);
            return;
        }
        if self.files_source_current() {
            if let Some(files) = &mut self.files {
                files.check_cached_revision |= files.visible && files.dirty(cx);
                files.visible = !files.visible;
                if !files.visible {
                    files.completion.state.dismiss();
                }
            }
            cx.notify();
            return;
        }
        if let Some(path) = self.prepare_files(cx) {
            self.list_files(path, cx);
        }
    }

    fn prepare_files(&mut self, cx: &mut Context<Self>) -> Option<String> {
        self.sync_file_source(cx);
        if self.files.as_ref().is_some_and(|f| f.busy) {
            if let Some(files) = &mut self.files {
                files.visible = true;
            }
            self.cancel_file_operation(cx);
            self.error = Some(
                "Wait for the original file operation to finish before changing sources.".into(),
            );
            cx.notify();
            return None;
        }
        if self.files_dirty(cx) {
            if let Some(files) = &mut self.files {
                files.visible = true;
            }
            self.error=Some("The original draft is retained. Explicitly discard it before opening Files for another source.".into());
            cx.notify();
            return None;
        }
        let (backend, preferred) = self.selected_document_backend(cx)?;
        let path = list::initial_folder(preferred, backend.is_wsl());
        self.initialize_file_panel(backend, cx);
        Some(path)
    }

    pub(super) fn initialize_file_panel(
        &mut self,
        backend: DocumentBackend,
        cx: &mut Context<Self>,
    ) {
        let epoch = self.source_selection_epoch();
        let location = Field::file_path(String::new(), cx);
        let layout = layout_prefs::FileLayout::read(cx);
        cx.observe(&location, |root, field, cx| {
            root.file_location_changed(field, cx)
        })
        .detach();
        self.files = Some(Files {
            backend,
            source_epoch: epoch,
            source: self.peers.selected.clone(),
            operation: None,
            resume_conflict: false,
            check_cached_revision: false,
            resume_retry: None,
            transfer: None,
            location,
            completion: Default::default(),
            browser_focus: completion::BrowserFocus::new(cx),
            listing: None,
            tree: tree::Tree::new(),
            listing_error: None,
            missing_folder: None,
            missing_focus: std::array::from_fn(|_| cx.focus_handle()),
            editor: None,
            preview: None,
            image_editor: None,
            document_path: None,
            document_error: None,
            visible: true,
            panel_focus: cx.focus_handle(),
            browser: true,
            browser_before_maximize: true,
            wrap: true,
            maximized: false,
            width: layout.width,
            browser_height: layout.browser_height,
            resize: None,
            width_focus: cx.focus_handle(),
            height_focus: cx.focus_handle(),
            confirm_focus: crate::files_view::confirmation::ConfirmationFocus::new(cx),
            busy: false,
            request: 0,
            pending_file_action: None,
            save_and_continue: false,
        });
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
        self.files.as_ref().is_some_and(|files| files.dirty(cx))
    }
}
#[cfg(test)]
mod tests;
