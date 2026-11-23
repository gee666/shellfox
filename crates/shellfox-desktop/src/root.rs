use crate::{
    dialogs::Dialog,
    terminal::TerminalView,
    app_host::{AppHost, CloseConfirmation, CloseTicket, WindowId},
    host_client::HostClient,
};
use gpui::{Context, Entity, FocusHandle, Window};
use shellfox_host::{SessionId, Snapshot, TabId};
use std::{
    collections::HashSet,
    path::PathBuf,
};

mod close;
mod quit_dialog;
pub(crate) use quit_dialog::QuitFocus;
mod catalog;
mod launch;
pub(crate) mod power;
mod semantics;
mod startup;

pub(crate) mod layout;
pub(crate) mod shortcuts;
pub(crate) mod panes;
pub struct StartupError {
    pub message: String,
    pub control_required: bool,
    /// Terminal generation the failure belongs to; a new generation retries the attach.
    pub generation: Option<shellfox_core::Generation>,
}
impl From<anyhow::Error> for StartupError {
    fn from(error: anyhow::Error) -> Self {
        let control_required = crate::terminal::control_required(&error);
        Self { message: format!("{error:#}"), control_required, generation: None }
    }
}
pub struct Root {
    pub(crate) host: Option<HostClient>,
    pub(crate) pending_boot: Option<crate::host_client::HostEndpointConfig>,
    /// The host model this window shows; the newest revision wins.
    pub(crate) snapshot: Option<Snapshot>,
    pub(crate) workspace: crate::workspace::WorkspaceState,
    pub(crate) tabs: crate::workspace::tabs::TabsState,
    pub(crate) sidebar: crate::sidebar::SidebarUi,
    pub(crate) files: crate::files::FilesArea,
    pub(crate) settings: crate::settings::SettingsSlot,
    pub(crate) dialogs: crate::dialogs::DialogState,
    pub(crate) peers: crate::peers::source::PeerSources,
    pub(crate) peer_ui: crate::peers::PeerUi,
    pub(crate) close: close::CloseState,
    pub(crate) semantics: semantics::Semantics,
    pub(crate) power: power::PowerUi,
    pub(crate) update_ui: crate::update::view::UpdateUi,
    pub(crate) window_tracker: Option<crate::desktop_geometry::WindowTracker>,
    pub(crate) fixture: Option<Entity<TerminalView>>,
    pub(crate) run_dir: PathBuf,
    pub(crate) error: Option<String>,
    pub(crate) notifications: crate::chrome::Notifications,
    pub(crate) layout_focus: FocusHandle,
    /// Host change subscriptions; dropped with the window.
    pub(crate) watches: Vec<gpui::Task<()>>,
    pub(crate) panes: panes::Panes,
}
impl Root {
    pub(crate) fn is_local_source(&self)->bool{
        self.peers.selected == crate::peers::source::Source::Local
    }
    /// UI publication cancellation, NOT a Manager/file authority epoch.
    pub(crate) fn source_selection_epoch(&self)->u64{
        self.peers.selection_epoch()
    }
    pub(crate) fn accept_snapshot(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) -> bool {
        if self
            .snapshot
            .as_ref()
            .is_some_and(|old| old.revision > snapshot.revision)
        {
            return false;
        }
        if !self.workspace.navigation.accept_snapshot(&snapshot) {
            return false;
        }
        let retained: HashSet<_> = snapshot
            .sessions
            .iter()
            .flat_map(|session| session.tabs.iter().map(|tab| tab.id))
            .collect();
        self.retire_wsl_file_tabs(&retained, cx);
        self.tabs.failures.retain(|id, _| retained.contains(id));
        self.tabs.views.retain(|id, _| retained.contains(id));
        for (tab, view) in &self.tabs.views {
            if let Some(session) = snapshot
                .sessions
                .iter()
                .find(|session| session.tabs.iter().any(|item| item.id == *tab))
            {
                let marker = session.settled_at.clone();
                view.update(cx, |view, cx| view.set_captured_settlement(marker, cx));
            }
        }
        crate::startup_chrome::StartupPalette::remember(&snapshot, cx);
        self.sidebar.snapshot(&snapshot, cx);
        self.snapshot = Some(snapshot);
        true
    }
    pub(crate) fn sync_dialog_edit_state(&self, cx: &mut Context<Self>) {
        match &self.dialogs.current {
            Some(Dialog::PeerSession(dialog)) => if let Some(field) = dialog.field() {
                field.update(cx, |field, cx| field.set_read_only(self.dialog_locked(), cx));
            },
            Some(Dialog::Create(draft)) => draft.path.update(cx, |field, cx| {
                field.set_read_only(self.dialogs.user_action_pending || draft.busy.is_some(), cx)
            }),
            Some(Dialog::RenameSession { title, .. } | Dialog::RenameTab { title, .. } | Dialog::SessionEnvironment { text: title, .. }) => title
                .update(cx, |field, cx| {
                    field.set_read_only(self.dialogs.user_action_pending, cx)
                }),
            _ => {}
        }
    }
    pub(crate) fn dialog_locked(&self) -> bool {
        self.dialogs.user_action_pending
            || matches!(self.dialogs.current, Some(Dialog::Quit))
                && (self.close.closing && !self.close.failed || self.close.check_pending)
            || matches!(&self.dialogs.current, Some(Dialog::Create(draft)) if draft.busy.is_some())
            || matches!(&self.dialogs.current, Some(Dialog::PeerSession(dialog)) if self.peer_ui.pending.contains(&dialog.cycle.peer))
    }
    pub(crate) fn dismiss_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog_locked() {
            return;
        }
        if matches!(self.dialogs.current, Some(Dialog::Quit)) {
            self.close.quit_focus.error = None;
            self.close.confirmation = None;
            self.close.ticket = None;
            self.close.update_restart = false;
            self.update_ui.restart_request = None;
        }
        if matches!(self.dialogs.current, Some(Dialog::PeerSession(_))) { self.dismiss_peer_session_dialog(window, cx); return; }
        let dialog = self.dialogs.current.take();
        if let Some(Dialog::RenameTab { id, .. }) = dialog {
            self.focus_tab_label(id, window, cx);
        } else if !matches!(dialog, Some(Dialog::Quit)) || !self.close.quit_focus.restore(window) {
            self.focus_selected(window, cx);
        }
        cx.notify();
    }
    pub(crate) fn selected_tab(&self) -> Option<TabId> {
        let selected_session = self.selected_session()?;
        let session = self.snapshot.as_ref()?.sessions.iter().find(|item| item.id == selected_session)?;
        let selected = self.workspace.navigation.selected().tab_id;
        let mut visible = session.tabs.iter().filter(|tab| self.local_tab_viewable(tab));
        visible.clone().find(|tab| Some(tab.id) == selected).or_else(|| visible.next()).map(|tab| tab.id)
    }
    pub(crate) fn selected_session(&self) -> Option<SessionId> {
        if !self.is_local_source(){return None;}
        self.workspace.navigation.selected().session_id
    }
    pub(crate) fn focus_selected(&self, window: &mut Window, cx: &gpui::App) {
        if !self.is_local_source() {
            if let Some(view) = self.peers.selected_peer().and_then(|(peer, state)| {
                self.peer_ui.v1_views.get(&(peer.clone(), state.selected_tab?))
            }) {
                view.read(cx).focus.focus(window);
            }
            return;
        }
        if let Some(view) = self.selected_tab().and_then(|id| self.tabs.views.get(&id)) {
            view.read(cx).focus.focus(window);
        }
    }
    /// Work whose inputs are window state. Runs after every render, which
    /// follows each change; time-based retries call notify_at.
    pub(crate) fn after_render(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.poll_peer_sources(window, cx);
        self.resume_peer_files(cx);
        if let Err(error) = self.sync_app_guard(cx) {
            let message = error.to_string();
            if self.error.as_ref() != Some(&message) {
                self.error = Some(message);
                cx.notify();
            }
        }
    }
    /// Re-renders once `deadline` has passed, so after_render sees it.
    pub(crate) fn notify_at(&self, deadline: std::time::Instant, cx: &mut Context<Self>) {
        cx.spawn(async move |root, cx| {
            let delay = deadline.saturating_duration_since(std::time::Instant::now());
            cx.background_executor().timer(delay).await;
            let _ = root.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }
}
