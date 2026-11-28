//! Whether the open Files panel and a finished file result still belong to
//! the selected source.
use super::{DocumentBackend, same_ui_source};
use crate::root::Root;
use gpui::Context;

impl Root {
    pub(crate) fn files_source_current(&self) -> bool {
        self.files.panel.as_ref().is_some_and(|f| {
            same_ui_source(
                f.source_epoch,
                self.source_selection_epoch(),
                f.backend.is_local(),
                self.is_local_source(),
            ) && f.source == self.peers.selected
                && (f.backend.is_local()
                    || self.peers.selected_peer().is_some_and(|(_, state)| {
                        state.phase == crate::peers::source::Phase::Connected
                    }))
                && self.active_file_tab(&f.backend)
                && f.backend.check_current().is_ok()
        })
    }
    pub(super) fn active_file_tab(&self, backend: &DocumentBackend) -> bool {
        backend
            .wsl_tab()
            .is_none_or(|tab| self.selected_tab() == Some(tab))
            && backend.peer_file_scope().is_none_or(|scope| {
                self.peers.selected_peer().is_some_and(|(_, state)| {
                    state
                        .selected_session
                        .is_some_and(|id| id.to_string() == scope.session.to_string())
                        && state
                            .selected_tab
                            .is_some_and(|id| id.to_string() == scope.tab.to_string())
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
            .panel
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
}
