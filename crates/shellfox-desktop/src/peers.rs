use crate::{dialogs::Dialog, root::Root, theme::Palette};
use gpui::{AnyElement, Context, Entity, Window, div, prelude::*, px};
use shellfox_protocol::{AuthenticatedPeerId, SessionId, TabId};
use std::collections::{HashMap, HashSet};
mod close;
mod connect;
mod connection_view;
pub(crate) mod metadata;
mod metadata_poll;
mod mutations;
mod pairing;
pub(crate) mod session_dialog;
pub(crate) mod sidebar;
mod sidebar_actions;
mod sidebar_layout;
pub(crate) mod source;
mod v1_stream;
mod workspace;
#[derive(Default)]
pub(crate) struct PeerUi {
    pub(crate) v1_views:
        HashMap<(AuthenticatedPeerId, TabId), Entity<crate::terminal::TerminalView>>,
    /// Last view that failed to open; the render hook skips it until Reconnect.
    pub(crate) v1_failed: Option<(AuthenticatedPeerId, TabId)>,
    v1_streams: HashMap<AuthenticatedPeerId, gpui::Task<()>>,
    /// Wakes the v1 streams when the UI unfreezes.
    stream_wake: std::sync::Arc<tokio::sync::Notify>,
    v1_activity: HashMap<(AuthenticatedPeerId, TabId), bool>,
    metadata_polls: HashMap<AuthenticatedPeerId, Option<std::time::Instant>>,
    catalog_reads: HashMap<AuthenticatedPeerId, metadata_poll::CatalogRead>,
    pub pending: HashSet<AuthenticatedPeerId>,
    opening: sidebar_actions::PeerActivations,
    pair_watches: HashMap<AuthenticatedPeerId, (uuid::Uuid, shellfox_protocol::CancellationToken)>,
    inventory_generation: u64,
    inventory_catalogs: HashSet<AuthenticatedPeerId>,
    legacy: HashMap<AuthenticatedPeerId, std::sync::Arc<crate::host_client::peer_v1::V1Binding>>,
    pub pending_create: Option<AuthenticatedPeerId>,
    pub collapsed: HashSet<AuthenticatedPeerId>,
    sidebar_layout:
        std::cell::RefCell<HashMap<AuthenticatedPeerId, sidebar_layout::PeerSectionLayout>>,
    pub pages: HashMap<AuthenticatedPeerId, usize>,
    pub navigation: HashMap<AuthenticatedPeerId, u64>,
    pub frozen: bool,
    pub withdrawn: bool,
}
impl PeerUi {}
impl Root {
    pub(crate) fn select_local_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.peer_ui.pending_create = None;
        if let Err(error) = self.peers.choose(source::Source::Local) {
            self.error = Some(error.to_string());
            return;
        }
        self.hide_v1_terminals(cx);
        self.focus_selected(window, cx);
        cx.notify();
    }
    /// Follows the host's remote state; the host holds each read until it changes.
    pub(crate) fn watch_peer_inventory(
        &self,
        client: crate::host_client::HostClient,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Task<()> {
        cx.spawn_in(window, async move |root, cx| {
            let mut changes = client.remote_changes();
            while let Ok(state) = changes.next().await {
                let current = root.update_in(cx, |root, window, cx| {
                    if !root.close.closing && !root.peer_ui.frozen {
                        root.accept_peer_remote(state, window, cx);
                    }
                    !root.close.closing
                });
                if !matches!(current, Ok(true)) {
                    return;
                }
            }
        })
    }
    pub(crate) fn poll_peer_inventory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.close.closing || self.peer_ui.frozen || self.peers.inventory_pending {
            return;
        }
        let Some(client) = self.host.clone() else {
            return;
        };
        self.peers.inventory_pending = true;
        let generation = self.peer_ui.inventory_generation;
        cx.spawn_in(window, async move |root, cx| {
            let reply = client
                .execute(shellfox_service::native::NativeAction::Remote(
                    shellfox_service::remote::native::RemoteRequest::State,
                ))
                .await;
            let _ = root.update_in(cx, |root, window, cx| {
                root.peers.inventory_pending = false;
                if generation == root.peer_ui.inventory_generation
                    && let Ok(shellfox_service::native::NativeResponse::Remote(state)) = reply
                {
                    root.accept_peer_remote(state, window, cx);
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
        if !state.enabled || self.close.closing || self.peer_ui.frozen {
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
                self.connect_catalog(id, window, cx);
            }
        }
    }

    pub(crate) fn choose_peer(
        &mut self,
        peer: AuthenticatedPeerId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.close.closing
            || self.peer_ui.frozen
            || self.dialog_locked()
            || self.dialogs.user_action_pending
        {
            return;
        }
        self.dialogs.current = None;
        self.sidebar.session_menu = None;
        self.tabs.profile_menu = None;
        self.hide_settings(cx);
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
        self.hide_v1_terminals(cx);
        self.connect_peer(peer, window, cx);
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
        self.follow_peer_file_tab(cx);
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
            .and_then(|snapshot| snapshot.sessions.first())
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
        session: SessionId,
        tab: Option<TabId>,
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
        let selection = |root: &Self| {
            root.peers
                .selected_peer()
                .map(|(peer, state)| (peer.clone(), state.selected_session, state.selected_tab))
        };
        let previous = selection(self);
        if let Err(error) = self.peers.select(&cycle, session, tab) {
            self.error = Some(error.to_string());
        } else {
            self.reveal_peer_session(&cycle, session);
            if selection(self) != previous {
                self.follow_peer_file_tab(cx);
            }
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
        let Some(state) = self.peers.peers.get(&peer) else {
            return;
        };
        if state.connection_action() != Some(source::ConnectionAction::Disconnect) {
            return;
        }
        let owner_connected = state.owner_connected;
        self.retire_peer_views(&peer, cx);
        if let Err(error) = self.peers.retire(&peer) {
            self.error = Some(error.to_string());
        }
        self.peer_ui.legacy.remove(&peer);
        self.retire_v1_terminals(&peer);
        if self.peer_ui.pending_create.as_ref() == Some(&peer) {
            self.peer_ui.pending_create = None;
        }
        let Some(host) = self.host.clone() else {
            return;
        };
        cx.spawn_in(window, async move |root, cx| {
            use shellfox_service::{
                native::{NativeAction, NativeResponse},
                remote::native::RemoteRequest,
            };
            let local = host.disconnect_peer(&peer).await;
            let owner = if owner_connected {
                host.execute(NativeAction::Remote(RemoteRequest::Disconnect {
                    peer_id: peer.as_str().into(),
                }))
                .await
                .map(Some)
            } else {
                Ok(None)
            };
            let _ = root.update_in(cx, |root, window, cx| {
                if let Err(error) = local {
                    root.error = Some(format!("Peer withdrawal failed: {error:#}"));
                }
                match owner {
                    Ok(Some(NativeResponse::Remote(state))) => {
                        root.accept_peer_remote(state, window, cx)
                    }
                    Err(error) => root.error = Some(format!("Peer withdrawal failed: {error:#}")),
                    _ => {}
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(crate) fn retire_peer_views(&mut self, peer: &AuthenticatedPeerId, cx: &mut Context<Self>) {
        // Terminal retirement: old views never resume after uncertain poll I/O.
        self.peer_ui.pending.remove(peer);
        self.peer_ui.opening.retire(peer);
        self.peer_ui.metadata_polls.remove(peer);
        self.peer_ui.catalog_reads.remove(peer);
        cx.notify();
    }
}
