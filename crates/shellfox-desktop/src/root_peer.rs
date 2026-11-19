use super::*;
use crate::theme::Palette;
use gpui::{AnyElement, div, prelude::*, px};
use shellfox_protocol::{AuthenticatedPeerId, manager_v2 as m};
#[cfg(feature = "experimental-shellfox-native")]
#[path = "peer_attach.rs"]
mod attach;
#[path = "peer_catalog_v1.rs"]
mod catalog_v1;
#[path = "peer_connect.rs"]
mod connect;
#[path = "peer_connection_view.rs"]
mod connection_view;
#[path = "peer_metadata.rs"]
pub(crate) mod metadata;
#[cfg(not(feature = "experimental-shellfox-native"))]
#[path = "peer_metadata_poll.rs"]
mod metadata_poll;
#[path = "peer_mutations.rs"]
mod mutations;
#[cfg(feature = "experimental-shellfox-native")]
#[path = "peer_poll.rs"]
mod poll;
#[path = "peer_session_dialog.rs"]
pub(crate) mod session_dialog;
#[path = "peer_sidebar.rs"]
pub(crate) mod sidebar;
#[path = "peer_sidebar_actions.rs"]
mod sidebar_actions;
#[path = "peer_sidebar_layout.rs"]
mod sidebar_layout;
#[cfg(feature = "experimental-shellfox-native")]
#[path = "peer_terminal.rs"]
pub(crate) mod terminal;
#[cfg(not(feature = "experimental-shellfox-native"))]
#[path = "peer_v1_stream.rs"]
mod v1_stream;
#[path = "peer_workspace.rs"]
mod workspace;
#[derive(Default)]
pub(crate) struct PeerUi {
    #[cfg(feature = "experimental-shellfox-native")]
    pub views: HashMap<(AuthenticatedPeerId, m::Id), Entity<terminal::PeerTerminalView>>,
    #[cfg(not(feature = "experimental-shellfox-native"))]
    pub(crate) v1_views: HashMap<(AuthenticatedPeerId, m::Id), Entity<crate::app::TerminalView>>,
    #[cfg(not(feature = "experimental-shellfox-native"))]
    v1_streams: HashMap<AuthenticatedPeerId, gpui::Task<()>>,
    #[cfg(not(feature = "experimental-shellfox-native"))]
    v1_activity: HashMap<(AuthenticatedPeerId, m::Id), bool>,
    #[cfg(feature = "experimental-shellfox-native")]
    polls: HashMap<AuthenticatedPeerId, poll::PollSlot>,
    #[cfg(not(feature = "experimental-shellfox-native"))]
    metadata_polls: HashMap<AuthenticatedPeerId, Option<std::time::Instant>>,
    pub paused: HashSet<AuthenticatedPeerId>,
    #[cfg(feature = "experimental-shellfox-native")]
    pub attaching: HashSet<AuthenticatedPeerId>,
    pub pending: HashSet<AuthenticatedPeerId>,
    inventory_catalogs: HashSet<AuthenticatedPeerId>,
    legacy: HashMap<AuthenticatedPeerId, std::sync::Arc<crate::host_client::peer_v1::V1Binding>>,
    pub pending_create: Option<AuthenticatedPeerId>,
    pub collapsed: HashSet<AuthenticatedPeerId>,
    sidebar_layout: std::cell::RefCell<HashMap<AuthenticatedPeerId, sidebar_layout::PeerSectionLayout>>,
    pub pages: HashMap<AuthenticatedPeerId, usize>,
    pub navigation: HashMap<AuthenticatedPeerId, u64>,
    #[cfg(feature = "experimental-shellfox-native")]
    pub recover: HashSet<(AuthenticatedPeerId, m::Id)>,
    pub frozen: bool,
    pub withdrawn: bool,
}
impl PeerUi {
    #[cfg(feature = "experimental-shellfox-native")]
    pub(crate) fn polls_drained(&self) -> bool {
        self.polls.is_empty()
    }
}
impl Root {
    #[cfg(feature = "experimental-shellfox-native")]
    pub(crate) fn peer_tab_busy(&self, _: &AuthenticatedPeerId, _: m::Id) -> Option<bool> {
        None
    }

    pub(crate) fn select_local_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.peer_ui.pending_create = None;
        if let Err(error) = self.peers.choose(source::Source::Local) {
            self.error = Some(error.to_string());
            return;
        }
        #[cfg(feature = "experimental-shellfox-native")]
        for view in self.peer_ui.views.values() {
            view.update(cx, |view, _| view.visible(false));
        }
        #[cfg(not(feature = "experimental-shellfox-native"))]
        self.hide_v1_terminals(cx);
        self.focus_selected(window, cx);
        cx.notify();
    }
    pub(crate) fn poll_peer_inventory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.closing_window || self.peer_ui.frozen || self.peers.inventory_pending {
            return;
        }
        let Some(client) = self.host.clone() else {
            return;
        };
        self.peers.inventory_pending = true;
        cx.spawn_in(window, async move |root, cx| {
            let reply = client
                .execute(shellfox_service::native::NativeAction::Remote(
                    shellfox_service::remote::native::RemoteRequest::State,
                ))
                .await;
            let _ = root.update_in(cx, |root, window, cx| {
                root.peers.inventory_pending = false;
                if let Ok(shellfox_service::native::NativeResponse::Remote(state)) = reply {
                    root.peers.inventory(&state.peers);
                    root.peer_ui.sidebar_layout.borrow_mut().retain(|peer, _| root.peers.peers.contains_key(peer));
                    root.warm_connected_catalogs(&state, window, cx);
                    root.update_pending_access(
                        state.pending.iter().map(|pending| {
                            (pending.peer.id.as_str(), pending.peer.hostname.as_str())
                        }),
                        cx,
                    );
                    cx.notify();
                }
            });
        })
        .detach();
    }
    // A connection opened in Settings or by another window also gets a sidebar catalog.
    fn warm_connected_catalogs(
        &mut self,
        state: &shellfox_service::remote::native::RemoteControlState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use shellfox_service::remote::native::RemoteConnection;
        if !state.enabled || self.closing_window || self.peer_ui.frozen {
            self.peer_ui.inventory_catalogs.clear();
            return;
        }
        self.peer_ui.inventory_catalogs.retain(|id| {
            state.peers.iter().any(|peer| {
                peer.id == id.as_str() && peer.connection == RemoteConnection::Connected
            })
        });
        for peer in &state.peers {
            if peer.connection != RemoteConnection::Connected {
                continue;
            }
            if let Ok(id) = AuthenticatedPeerId::new(peer.id.clone())
                && let Some(source) = self.peers.peers.get(&id)
                && !source.pending
                && self.peer_ui.inventory_catalogs.insert(id.clone())
                && source.phase != source::Phase::Connected
            {
                self.connect_peer(id, false, window, cx);
            }
        }
    }

    pub(crate) fn choose_peer(
        &mut self,
        peer: AuthenticatedPeerId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.closing_window
            || self.peer_ui.frozen
            || self.dialog_locked()
            || self.user_action_pending
        {
            return;
        }
        self.dialog = None;
        self.session_menu = None;
        self.profile_menu = None;
        #[cfg(feature = "experimental-shellfox-native")]
        for view in self.peer_ui.views.values() {
            view.update(cx, |view, _| view.visible(false));
        }
        self.settings = false;
        if let Err(error) = self.peers.choose(source::Source::Peer(peer.clone())) {
            self.error = Some(error.to_string());
            return;
        }
        if self.peers.peers.get(&peer).is_some_and(|state| {
            state.phase == source::Phase::Connected && state.snapshot.is_some()
        }) {
            self.resume_peer_selection(&peer, window, cx);
            cx.notify();
            return;
        }
        #[cfg(not(feature = "experimental-shellfox-native"))]
        self.hide_v1_terminals(cx);
        self.connect_peer(peer, false, window, cx);
    }
    fn resume_peer_selection(
        &mut self,
        peer: &AuthenticatedPeerId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.peers.selected != source::Source::Peer(peer.clone()) {
            return;
        }
        let Some(state) = self.peers.peers.get(peer) else {
            return;
        };
        if state.phase != source::Phase::Connected {
            return;
        }
        if state.selected_session.is_some() {
            self.ensure_peer_terminal(window, cx);
            return;
        }
        if state.selection_committed {
            return;
        }
        let Some(id) = state
            .snapshot
            .as_ref()
            .and_then(|snapshot| source::sessions(snapshot).next())
            .map(|session| session.id)
        else {
            return;
        };
        self.select_peer_session(
            source::PeerCycle {
                peer: peer.clone(),
                generation: state.cycle,
            },
            id,
            None,
            window,
            cx,
        );
    }

    pub(crate) fn select_peer_session(
        &mut self,
        cycle: source::PeerCycle,
        session: m::Id,
        tab: Option<m::Id>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let nav = self
            .peer_ui
            .navigation
            .entry(cycle.peer.clone())
            .or_default();
        let Some(next) = nav.checked_add(1) else {
            self.error = Some("Peer navigation epoch exhausted.".into());
            return;
        };
        *nav = next;
        if let Err(error) = self.peers.select(&cycle, session, tab) {
            self.error = Some(error.to_string());
        } else {
            self.reveal_peer_session(&cycle, session);
            self.ensure_peer_terminal(window, cx);
        }
        cx.notify();
    }
    pub(crate) fn disconnect_peer(
        &mut self,
        peer: AuthenticatedPeerId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.retire_peer_views(&peer, cx);
        if let Err(error) = self.peers.retire(&peer) {
            self.error = Some(error.to_string());
        }
        self.peer_ui.legacy.remove(&peer);
        #[cfg(not(feature = "experimental-shellfox-native"))]
        self.retire_v1_terminals(&peer);
        if self.peer_ui.pending_create.as_ref() == Some(&peer) {
            self.peer_ui.pending_create = None;
        }
        let Some(host) = self.host.clone() else {
            return;
        };
        cx.spawn_in(window, async move |root, cx| {
            let result = host.disconnect_peer(&peer).await;
            let _ = root.update_in(cx, |root, _, cx| {
                if let Err(error) = result {
                    root.error = Some(format!("Peer withdrawal failed: {error:#}"));
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(crate) fn retire_peer_views(&mut self, peer: &AuthenticatedPeerId, cx: &mut Context<Self>) {
        // Terminal retirement: old views never resume after uncertain poll I/O.
        self.peer_ui.paused.insert(peer.clone());
        self.peer_ui.pending.remove(peer);
        #[cfg(not(feature = "experimental-shellfox-native"))]
        self.peer_ui.metadata_polls.remove(peer);
        #[cfg(feature = "experimental-shellfox-native")]
        {
            self.peer_ui.polls.remove(peer);
            self.peer_ui.attaching.remove(peer);
            self.peer_ui.recover.retain(|(source, _)| source != peer);
            self.peer_ui.views.retain(|(source, _), view| {
                if source == peer {
                    view.update(cx, |view, _| view.retire());
                    false
                } else {
                    true
                }
            });
        }
        cx.notify();
    }
}
