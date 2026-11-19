use super::{DocumentBackend, Files, image_editor::ImageEditor};
use crate::root::Root;
use gpui::{App, Context};

impl Files {
    pub(super) fn dirty(&self, cx: &App) -> bool {
        self.editor.as_ref().is_some_and(|editor| editor.dirty(cx))
            || self.image_editor.as_ref().is_some_and(ImageEditor::dirty)
    }

    fn suspend_presentation(&mut self, cx: &mut Context<Root>) {
        self.completion.state.dismiss();
        self.resize = None;
        self.pending_file_action = None;
        self.save_and_continue = false;
        self.confirm_focus.forget();
        if self.busy {
            self.request += 1;
            self.busy = false;
            self.location
                .update(cx, |field, cx| field.set_read_only(false, cx));
            if let Some(operation) = self.operation.take() {
                operation.cancel();
                if let Some(editor) = &mut self.editor {
                    editor.reload_required = true;
                    self.document_error =
                        Some("Save was not confirmed. Reload before saving again.".into());
                } else if let Some(ImageEditor::Ready(draft)) = &mut self.image_editor {
                    draft.reload_required = true;
                    draft.error =
                        Some("Image save was not confirmed. Reload before saving again.".into());
                } else {
                    self.document_error =
                        Some("File load was cancelled when switching hosts.".into());
                }
            }
        }
        if let Some(image) = &mut self.image_editor {
            match image {
                ImageEditor::Loading => self.image_editor = None,
                ImageEditor::Ready(draft) => {
                    draft.working = false;
                    draft.anchor = None;
                }
                ImageEditor::Failed(_) => {}
            }
        }
        self.tree.cancel_loading();
    }
}

impl Root {
    /// Move only presentation state. Peer I/O retains its original admission and is never rebound.
    pub(crate) fn sync_file_source(&mut self, cx: &mut Context<Self>) {
        let source = self.peers.selected.clone();
        let epoch = self.source_selection_epoch();
        if self
            .files
            .as_ref()
            .is_some_and(|files| files.source == source && files.source_epoch == epoch)
        {
            return;
        }
        self.remember_panel(cx);
        if let Some(index) = self
            .retained_files
            .iter()
            .rposition(|files| files.source == source)
        {
            self.restore_panel(index);
        }
    }

    pub(crate) fn select_local_file_scope(
        &mut self,
        backend: DocumentBackend,
        cx: &mut Context<Self>,
    ) {
        if !self.is_local_source() || !backend.is_local() {
            return;
        }
        self.sync_file_source(cx);
        if !self
            .files
            .as_ref()
            .is_some_and(|files| files.backend.same_panel_scope(&backend))
        {
            self.remember_panel(cx);
            let source = &self.peers.selected;
            if let Some(index) = self.retained_files.iter().rposition(|files| {
                files.source == *source && files.backend.same_panel_scope(&backend)
            }) {
                self.restore_panel(index);
            } else {
                self.initialize_file_panel(backend, cx);
            }
        }
        if let Some(files) = &mut self.files {
            files.visible = true;
        }
    }

    pub(crate) fn follow_wsl_file_tab(&mut self, cx: &mut Context<Self>) {
        if self.is_local_source() && self.files.as_ref().is_some_and(|files| files.visible && files.backend.is_wsl()) {
            self.show_selected_file_scope(cx);
        }
    }

    pub(super) fn show_selected_file_scope(&mut self, cx: &mut Context<Self>) {
        let Some((backend, preferred)) = self.selected_document_backend(cx) else {
            self.remember_panel(cx);
            cx.notify();
            return;
        };
        self.select_local_file_scope(backend, cx);
        if self.files.as_ref().is_some_and(|files| files.listing.is_none() && !files.busy) {
            self.list_files(preferred.unwrap_or_else(|| "~".into()), cx);
        }
    }

    pub(crate) fn wsl_tab_has_draft(&self, tab: shellfox_core::TabId, cx: &App) -> bool {
        self.files.iter().chain(&self.retained_files).any(|files| {
            files.backend.wsl_tab() == Some(tab) && (files.dirty(cx) || files.busy)
        })
    }

    pub(crate) fn retire_wsl_file_tabs(&mut self, retained: &std::collections::HashSet<shellfox_core::TabId>, cx: &mut Context<Self>) {
        let retired = |files: &Files| files.backend.wsl_tab().is_some_and(|tab| !retained.contains(&tab));
        let clean = |files: &Files| retired(files) && !files.dirty(cx);
        for files in self.files.iter_mut().chain(&mut self.retained_files).filter(|files| retired(files)) {
            files.backend.retire_wsl_editor();
            if let Some(editor) = &mut files.editor {
                editor.reload_required = true;
            }
            files.document_error = Some("The terminal tab closed. The draft has not been saved.".into());
        }
        if self.files.as_ref().is_some_and(clean) {
            self.files.take();
        }
        self.retained_files.retain(|files| !clean(files));
    }

    pub(crate) fn use_host_file_scope(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(super::EditorOutcome::Ready(backend)) = cx.try_global::<super::EditorOutcome>()
        else {
            return false;
        };
        let backend = DocumentBackend::local(backend);
        self.select_local_file_scope(backend, cx);
        true
    }

    fn restore_panel(&mut self, index: usize) {
        let mut files = self.retained_files.remove(index);
        files.source_epoch = self.source_selection_epoch();
        self.files = Some(files);
    }

    fn remember_panel(&mut self, cx: &mut Context<Self>) {
        if let Some(mut files) = self.files.take() {
            files.suspend_presentation(cx);
            self.retained_files.retain(|retained| {
                retained.source != files.source
                    || !retained.backend.same_panel_scope(&files.backend)
            });
            self.retained_files.push(files);
        }
    }
}
