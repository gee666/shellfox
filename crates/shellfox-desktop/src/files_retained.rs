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
        // A save already owns its destination. Hiding its panel must not cancel it
        // or discard its receipt. Reads, on the other hand, are presentation work.
        if self.busy && !self.saving() {
            self.request += 1;
            self.busy = false;
            self.location
                .update(cx, |field, cx| field.set_read_only(false, cx));
            if let Some(operation) = self.operation.take() {
                if matches!(operation, super::FileOperation::Open(_)) {
                    self.document_error = Some("File load was cancelled when switching scopes.".into());
                }
                operation.cancel();
            }
        }
        if let Some(ImageEditor::Ready(draft)) = &mut self.image_editor {
            draft.anchor = None;
        }
        self.tree.cancel_loading();
    }
}

impl Root {
    pub(super) fn file_write_recipient(
        &mut self,
        request: u64,
        backend: &DocumentBackend,
    ) -> Option<(&mut Files, bool)> {
        let matches = |files: &Files| {
            files.request == request && files.saving() && files.backend.same_origin(backend)
        };
        if self.files.as_ref().is_some_and(matches) {
            let presented = self.files_source_current();
            return self.files.as_mut().map(|files| {
                let presented = presented && files.visible;
                (files, presented)
            });
        }
        self.retained_files.iter_mut().find(|files| matches(files)).map(|files| (files, false))
    }

    /// Move only presentation state. Reconnecting separately revalidates the file.
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
        let active = self.retained_files.iter().rposition(|files| {
            files.source == source && self.active_file_tab(&files.backend)
        });
        if let Some(index) = active.or_else(|| self.retained_files.iter().rposition(|files| files.source == source)) {
            self.restore_panel(index);
        }
    }

    pub(crate) fn select_file_scope(
        &mut self,
        backend: DocumentBackend,
        cx: &mut Context<Self>,
    ) {
        if self.is_local_source() != backend.is_local() {
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
        self.select_file_scope(backend, cx);
        if self.files.as_ref().is_some_and(|files| files.listing.is_none() && !files.busy) {
            self.list_files(preferred.unwrap_or_else(|| "~".into()), cx);
        }
    }

    pub(crate) fn follow_peer_file_tab(&mut self, cx: &mut Context<Self>) {
        self.sync_file_source(cx);
        if !self.files.as_ref().is_some_and(|files| files.visible && files.source == self.peers.selected) {
            return;
        }
        let Ok((backend, preferred)) = self.peer_file_backend() else { return; };
        if backend.is_wsl() || self.files.as_ref().is_some_and(|files| files.backend.is_wsl()) {
            self.select_file_scope(backend, cx);
            if self.files.as_ref().is_some_and(|files| files.listing.is_none() && !files.busy) {
                self.list_files(preferred.unwrap_or_else(|| "~".into()), cx);
            }
        }
    }

    pub(crate) fn reconcile_peer_file_catalog(
        &mut self,
        peer: &shellfox_protocol::AuthenticatedPeerId,
        cx: &mut Context<Self>,
    ) {
        use crate::root::source::{Source, viewable_tab};
        let Some(snapshot) = self.peers.peers.get(peer).and_then(|state| state.snapshot.as_ref()) else { return };
        let source = Source::Peer(peer.clone());
        let removable = |files: &Files| {
            files.source == source && !files.dirty(cx) && !files.busy && files.transfer.is_none()
                && !files.image_editor.as_ref().is_some_and(ImageEditor::transforming)
                && files.backend.peer_file_scope().is_some_and(|scope| {
                    !snapshot.sessions.iter().any(|session| session.id == scope.session
                        && session.tabs.iter().any(|tab| tab.id == scope.tab && viewable_tab(tab)))
                })
        };
        self.retained_files.retain(|files| !removable(files));
        if self.files.as_ref().is_some_and(removable) {
            let visible = self.files.take().is_some_and(|files| files.visible);
            if visible && self.peers.selected == source {
                self.show_selected_file_scope(cx);
            }
        }
    }

    pub(crate) fn reveal_dirty_file_on_close(&mut self, cx: &mut Context<Self>) {
        if !self.files.as_ref().is_some_and(|files| files.dirty(cx)) {
            let Some(index) = self.retained_files.iter().position(|files| files.dirty(cx)) else { return };
            let original = self.retained_files.remove(index);
            self.remember_panel(cx);
            self.files = Some(original);
        }
        let Some(source) = self.files.as_ref().map(|files| files.source.clone()) else { return };
        if self.peers.choose(source).is_ok() {
            let epoch = self.source_selection_epoch();
            if let Some(files) = &mut self.files {
                files.source_epoch = epoch;
                files.visible = true;
            }
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
        let backend = if self.is_local_source() {
            let Some(super::EditorOutcome::Ready(backend)) = cx.try_global::<super::EditorOutcome>() else { return false; };
            DocumentBackend::local(backend)
        } else {
            let Ok((backend, _)) = self.peer_file_backend_for(false) else { return false; };
            backend
        };
        self.select_file_scope(backend, cx);
        true
    }

    fn restore_panel(&mut self, index: usize) {
        let mut files = self.retained_files.remove(index);
        files.source_epoch = self.source_selection_epoch();
        self.files = Some(files);
    }

    fn remember_panel(&mut self, cx: &mut Context<Self>) {
        if let Some(mut files) = self.files.take() {
            files.check_cached_revision |= files.dirty(cx);
            files.suspend_presentation(cx);
            self.retained_files.retain(|retained| {
                retained.source != files.source
                    || !retained.backend.same_panel_scope(&files.backend)
            });
            self.retained_files.push(files);
        }
    }
}
