use super::*;

impl Root {
    pub(crate) fn select_session(
        &mut self,
        id: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closing_window {
            return;
        }
        if !self.is_local_source() {
            self.select_local_source(window, cx);
        }
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        if !self.navigation.select_session(snapshot, id) {
            return;
        }
        self.hide_settings(cx);
        let fence = self.local_activation_fence();
        self.pending_recent = (self.activating_session != Some((id, fence)))
            .then_some((RecentSelection::Session(id), fence.source));
        self.follow_wsl_file_tab(cx);
        self.reveal_sidebar_session(id, cx);
        self.ensure_terminal(window, cx);
        self.flush_recent(window, cx);
        cx.notify();
    }
    pub(crate) fn select_tab(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing_window {
            return;
        }
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let Some(session) = snapshot
            .sessions
            .iter()
            .find(|session| session.tabs.iter().any(|tab| tab.id == id))
        else {
            return;
        };
        if !self.navigation.select_tab(snapshot, session.id, id) {
            return;
        }
        self.pending_recent = Some((RecentSelection::Tab(id), self.source_selection_epoch()));
        self.follow_wsl_file_tab(cx);
        self.ensure_terminal(window, cx);
        self.flush_recent(window, cx);
        cx.notify();
    }
    pub(crate) fn flush_recent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.action_pending || self.background.remember || self.closing_window {
            return;
        }
        let Some((recent, source_epoch)) = self.pending_recent.take() else {
            return;
        };
        if !self.is_local_source() || self.source_selection_epoch() != source_epoch {
            return;
        }
        let action = match recent {
            RecentSelection::Session(id) => {
                NativeAction::Metadata(HostRequest::ActivateSession { session_id: id })
            }
            RecentSelection::Tab(id) => NativeAction::SelectTab { tab: id },
        };
        self.submit_native(action, AfterAction::Remember, window, cx);
    }
}
