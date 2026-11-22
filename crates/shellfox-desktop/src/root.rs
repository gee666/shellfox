use crate::{
    app::TerminalView,
    app_host::{AppHost, CloseConfirmation, CloseTicket, WindowId},
    field::Field,
    host_client::HostClient,
};
use gpui::{Context, Entity, FocusHandle, Window};
use shellfox_host::{SessionId, Snapshot, TabId};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

#[path = "root_close.rs"]
mod close;
pub(crate) use close::log_shutdown_report;
#[path = "quit_dialog.rs"]
mod quit_dialog;
pub(crate) use quit_dialog::QuitFocus;
#[path = "root_catalog.rs"]
mod catalog;
#[path = "root_launch.rs"]
mod launch;
#[path = "root_power.rs"]
pub(crate) mod power;
#[path = "root_semantics.rs"]
mod semantics;
#[path = "root_startup.rs"]
mod startup;
#[path="root_peer_close.rs"]mod peer_close;
#[path = "peer_source.rs"] pub(crate) mod source;
#[path = "root_peer.rs"] pub(crate) mod peer;

pub struct StartupError {
    pub message: String,
    pub control_required: bool,
}
impl From<anyhow::Error> for StartupError {
    fn from(error: anyhow::Error) -> Self {
        let control_required = error.downcast_ref::<shellfox_runtime::RuntimeError>()
            .is_some_and(|error| error.code == shellfox_runtime::ErrorCode::ControlRequired);
        Self { message: format!("{error:#}"), control_required }
    }
}
pub enum Dialog {
    Create(crate::directory::Draft),
    RenameSession { id: SessionId, title: Entity<Field> },
    SessionEnvironment { id: SessionId, text: Entity<Field> },
    PeerSession(peer::session_dialog::PeerSessionDialog),
    RenameTab { id: TabId, title: Entity<Field>, _blur: gpui::Subscription },
    DeleteSession(SessionId),
    CloseTab(shellfox_core::TerminalIdentity),
    Quit,
}
pub struct Root {
    pub(crate) host: Option<HostClient>,
    pub(crate) tab_activity: crate::tab_activity::TabActivity,
    pub(crate) peers: source::PeerSources,
    pub(crate) peer_ui: peer::PeerUi,
    pub(crate) semantic_context: crate::accessibility::WindowSemanticContext,
    pub(crate) native_accessibility: Option<crate::accessibility::NativeAccessibility>,
    pub(crate) pending_boot: Option<crate::host_client::HostEndpointConfig>,
    pub(crate) app_host: Option<AppHost>,
    pub(crate) window_id: Option<WindowId>,
    pub(crate) close_confirmation: Option<CloseConfirmation>,
    pub(crate) quit_focus: QuitFocus,
    pub(crate) window_tracker: Option<crate::desktop_geometry::WindowTracker>,
    pub(crate) close_ticket: Option<CloseTicket>,
    pub(crate) update_restart: bool,
    pub(crate) update_ui: crate::update_view::UpdateUi,
    pub(crate) close_check_pending: bool,
    pub(crate) close_failed: bool,
    pub(crate) snapshot: Option<Snapshot>,
    pub(crate) navigation: crate::window_manager::WindowSelection,
    pub(crate) tab_scroll: crate::chrome::TabScroll,
    pub(crate) pending_recent: Option<(crate::workspace::RecentSelection, u64)>,
    pub(crate) activating_session: Option<(SessionId, crate::workspace::ActivationFence)>,
    pub(crate) views: HashMap<TabId, Entity<TerminalView>>,
    pub(crate) attaching: HashSet<TabId>,
    pub(crate) failures: HashMap<TabId, StartupError>,
    pub(crate) settings_panel: Option<Entity<crate::settings::SettingsPanel>>,
    pub(crate) files: Option<crate::files::Files>,
    pub(crate) html_preview: crate::files::text_preview::html::Surface,
    pub(crate) retained_files: Vec<crate::files::Files>,
    pub(crate) fixture: Option<Entity<TerminalView>>,
    pub(crate) run_dir: PathBuf,
    pub(crate) window_caption: String,
    pub(crate) error: Option<String>,
    pub(crate) notifications: crate::chrome::Notifications,
    pub(crate) dialog: Option<Dialog>,
    pub(crate) settings: bool,
    pub(crate) sidebar: bool,
    pub(crate) layout_focus: FocusHandle,
    pub(crate) sidebar_width: f32,
    pub(crate) resizing_sidebar: bool,
    pub(crate) local_expanded: bool,
    pub(crate) history_page_size: usize,
    pub(crate) session_pages: usize,
    pub(crate) sidebar_ui: crate::sidebar::SidebarUi,
    pub(crate) session_menu: Option<crate::session_menu::SessionMenu>,
    pub(crate) tab_menu: Option<crate::chrome::TabMenu>,
    pub(crate) modal_focus: FocusHandle,
    pub(crate) closing_window: bool,
    pub(crate) action_pending: bool,
    pub(crate) user_action_pending: bool,
    pub(crate) background: crate::workspace::Background,
    pub(crate) profiles: Vec<crate::settings::api::Profile>,
    pub(crate) catalog_epoch: u64,
    pub(crate) profile_menu: Option<crate::profile_menu::ProfileMenu>,
    pub(crate) power: power::PowerUi,
    /// Host change subscriptions; dropped with the window.
    pub(crate) watches: Vec<gpui::Task<()>>,
}
impl Root {
    pub(crate) fn is_local_source(&self)->bool{
        self.peers.selected==source::Source::Local
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
        if !self.navigation.accept_snapshot(&snapshot) {
            return false;
        }
        let retained: HashSet<_> = snapshot
            .sessions
            .iter()
            .flat_map(|session| session.tabs.iter().map(|tab| tab.id))
            .collect();
        self.retire_wsl_file_tabs(&retained, cx);
        self.failures.retain(|id, _| retained.contains(id));
        self.views.retain(|id, _| retained.contains(id));
        for (tab, view) in &self.views {
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
        self.sidebar_ui.snapshot(&snapshot, cx);
        self.snapshot = Some(snapshot);
        true
    }
    pub(crate) fn sync_dialog_edit_state(&self, cx: &mut Context<Self>) {
        match &self.dialog {
            Some(Dialog::PeerSession(dialog)) => if let Some(field) = dialog.field() {
                field.update(cx, |field, cx| field.set_read_only(self.dialog_locked(), cx));
            },
            Some(Dialog::Create(draft)) => draft.path.update(cx, |field, cx| {
                field.set_read_only(self.user_action_pending || draft.busy.is_some(), cx)
            }),
            Some(Dialog::RenameSession { title, .. } | Dialog::RenameTab { title, .. } | Dialog::SessionEnvironment { text: title, .. }) => title
                .update(cx, |field, cx| {
                    field.set_read_only(self.user_action_pending, cx)
                }),
            _ => {}
        }
    }
    pub(crate) fn dialog_locked(&self) -> bool {
        self.user_action_pending
            || matches!(self.dialog, Some(Dialog::Quit))
                && (self.closing_window && !self.close_failed || self.close_check_pending)
            || matches!(&self.dialog, Some(Dialog::Create(draft)) if draft.busy.is_some())
            || matches!(&self.dialog, Some(Dialog::PeerSession(dialog)) if self.peer_ui.pending.contains(&dialog.cycle.peer))
    }
    pub(crate) fn dismiss_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog_locked() {
            return;
        }
        if matches!(self.dialog, Some(Dialog::Quit)) {
            self.quit_focus.error = None;
            self.close_confirmation = None;
            self.close_ticket = None;
            self.update_restart = false;
            self.update_ui.restart_request = None;
        }
        if matches!(self.dialog, Some(Dialog::PeerSession(_))) { self.dismiss_peer_session_dialog(window, cx); return; }
        let dialog = self.dialog.take();
        if let Some(Dialog::RenameTab { id, .. }) = dialog {
            self.focus_tab_label(id, window, cx);
        } else if !matches!(dialog, Some(Dialog::Quit)) || !self.quit_focus.restore(window) {
            self.focus_selected(window, cx);
        }
        cx.notify();
    }
    pub(crate) fn selected_tab(&self) -> Option<TabId> {
        let selected_session = self.selected_session()?;
        let session = self.snapshot.as_ref()?.sessions.iter().find(|item| item.id == selected_session)?;
        let selected = self.navigation.selected().tab_id;
        let mut visible = session.tabs.iter().filter(|tab| self.local_tab_viewable(tab));
        visible.clone().find(|tab| Some(tab.id) == selected).or_else(|| visible.next()).map(|tab| tab.id)
    }
    pub(crate) fn selected_session(&self) -> Option<SessionId> {
        if !self.is_local_source(){return None;}
        self.navigation.selected().session_id
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
        if let Some(view) = self.selected_tab().and_then(|id| self.views.get(&id)) {
            view.read(cx).focus.focus(window);
        }
    }
    fn start_poll(window: &Window, cx: &Context<Self>) {
        cx.spawn_in(window, async move |root, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(250))
                    .await;
                if root
                    .update_in(cx, |root, window, cx| {
                        root.poll_session_visibility(window, cx);
                        root.poll_peer_inventory(window, cx);
                        root.poll_peer_sources(window, cx);
                        root.resume_peer_files(cx);
                        if let Err(error) = root.sync_app_guard(cx) {
                            root.error = Some(error.to_string());
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }
}
