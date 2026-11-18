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
#[cfg(feature = "experimental-shellfox-native")]
#[path = "peer_source.rs"] pub(crate) mod source;
#[cfg(feature = "experimental-shellfox-native")]
#[path = "root_peer.rs"] pub(crate) mod peer;

pub struct StartupError {
    pub message: String,
}
impl From<anyhow::Error> for StartupError {
    fn from(error: anyhow::Error) -> Self {
        Self {
            message: format!("{error:#}"),
        }
    }
}
pub enum Dialog {
    Create(crate::directory::Draft),
    RenameSession { id: SessionId, title: Entity<Field> },
    RenameTab { id: TabId, title: Entity<Field> },
    DeleteSession(SessionId),
    CloseTab(TabId),
    Quit,
}
pub struct Root {
    pub(crate) host: Option<HostClient>,
    #[cfg(feature = "experimental-shellfox-native")]
    pub(crate) peers: source::PeerSources,
    #[cfg(feature = "experimental-shellfox-native")]
    pub(crate) peer_ui: peer::PeerUi,
    pub(crate) semantic_context: crate::accessibility::WindowSemanticContext,
    pub(crate) native_accessibility: Option<crate::accessibility::NativeAccessibility>,
    pub(crate) pending_boot: Option<crate::host_client::HostEndpointConfig>,
    pub(crate) app_host: Option<AppHost>,
    pub(crate) window_id: Option<WindowId>,
    pub(crate) close_confirmation: Option<CloseConfirmation>,
    pub(crate) close_ticket: Option<CloseTicket>,
    pub(crate) close_check_pending: bool,
    pub(crate) close_failed: bool,
    pub(crate) snapshot: Option<Snapshot>,
    pub(crate) navigation: crate::window_manager::WindowSelection,
    pub(crate) tab_scroll: crate::chrome::TabScroll,
    pub(crate) pending_recent: Option<crate::workspace::RecentSelection>,
    pub(crate) views: HashMap<TabId, Entity<TerminalView>>,
    pub(crate) attaching: HashSet<TabId>,
    pub(crate) failures: HashMap<TabId, StartupError>,
    pub(crate) settings_panel: Option<Entity<crate::settings::SettingsPanel>>,
    pub(crate) files: Option<crate::files::Files>,
    pub(crate) fixture: Option<Entity<TerminalView>>,
    pub(crate) run_dir: PathBuf,
    pub(crate) window_caption: String,
    pub(crate) error: Option<String>,
    pub(crate) dialog: Option<Dialog>,
    pub(crate) settings: bool,
    pub(crate) sidebar: bool,
    pub(crate) sidebar_width: f32,
    pub(crate) resizing_sidebar: bool,
    pub(crate) local_expanded: bool,
    pub(crate) history_page_size: usize,
    pub(crate) session_pages: usize,
    pub(crate) session_menu: Option<(SessionId, gpui::Point<gpui::Pixels>)>,
    pub(crate) modal_focus: FocusHandle,
    pub(crate) closing_window: bool,
    pub(crate) action_pending: bool,
    pub(crate) user_action_pending: bool,
    pub(crate) refresh_admission: crate::workspace::refresh::RefreshAdmission,
    pub(crate) profiles: Vec<crate::settings::api::Profile>,
    pub(crate) catalog_epoch: u64,
    pub(crate) ssh_ui: crate::root_ssh::SshUi,
    pub(crate) profile_menu: Option<crate::profile_menu::ProfileMenu>,
    pub(crate) power: power::PowerUi,
}
impl Root {
    pub(crate) fn is_local_source(&self)->bool{
        #[cfg(feature="experimental-shellfox-native")] {self.peers.selected==source::Source::Local}
        #[cfg(not(feature="experimental-shellfox-native"))] {true}
    }
    /// UI publication cancellation, NOT a Manager/file authority epoch.
    pub(crate) fn source_selection_epoch(&self)->u64{
        #[cfg(feature="experimental-shellfox-native")] {self.peers.selection_epoch()}
        #[cfg(not(feature="experimental-shellfox-native"))] {0}
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
        self.snapshot = Some(snapshot);
        true
    }
    pub(crate) fn sync_dialog_edit_state(&self, cx: &mut Context<Self>) {
        match &self.dialog {
            Some(Dialog::Create(draft)) => draft.path.update(cx, |field, cx| {
                field.set_read_only(self.user_action_pending || draft.busy, cx)
            }),
            Some(Dialog::RenameSession { title, .. } | Dialog::RenameTab { title, .. }) => title
                .update(cx, |field, cx| {
                    field.set_read_only(self.user_action_pending, cx)
                }),
            _ => {}
        }
    }
    pub(crate) fn dialog_locked(&self) -> bool {
        self.user_action_pending || matches!(&self.dialog,Some(Dialog::Create(draft))if draft.busy)
    }
    pub(crate) fn dismiss_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog_locked() {
            return;
        }
        if matches!(self.dialog, Some(Dialog::Quit)) {
            self.close_confirmation = None;
            self.close_ticket = None;
        }
        self.dialog = None;
        self.focus_selected(window, cx);
        cx.notify();
    }
    pub(crate) fn selected_tab(&self) -> Option<TabId> {
        if !self.is_local_source(){return None;}
        self.navigation.selected().tab_id
    }
    pub(crate) fn selected_session(&self) -> Option<SessionId> {
        if !self.is_local_source(){return None;}
        self.navigation.selected().session_id
    }
    pub(crate) fn focus_selected(&self, window: &mut Window, cx: &gpui::App) {
        #[cfg(feature="experimental-shellfox-native")]
        if let Some((peer,state))=self.peers.selected_peer(){if let Some(view)=state.selected_tab.and_then(|tab|self.peer_ui.views.get(&(peer.clone(),tab))){view.read(cx).focus(window);}return;}
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
                        root.poll_semantics(window, cx);
                        root.poll_ssh(window, cx);
                        root.poll_power(window, cx);
                        #[cfg(feature="experimental-shellfox-native")] {root.poll_peer_inventory(window,cx);root.poll_peer_sources(window,cx);}
                        if let Err(error) = root.sync_app_guard(cx) {
                            root.error = Some(error.to_string());
                            cx.notify();
                        }
                        if !root.closing_window && !root.action_pending && root.host.is_some() {
                            if root.pending_recent.is_some() {
                                root.flush_recent(window, cx);
                            } else {
                                root.refresh_snapshot(window, cx);
                            }
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
