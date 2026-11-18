use super::*;
use crate::{field::Field, theme::Palette};
use gpui::{AnyElement, div, prelude::*, px};
use shellfox_protocol::{AuthenticatedPeerId, manager_v2 as m};
#[path = "peer_attach.rs"]
mod attach;
#[path = "peer_mutations.rs"]
mod mutations;
#[path = "peer_poll.rs"]
mod poll;
#[path = "peer_sidebar.rs"]
mod sidebar;
#[path = "peer_terminal.rs"]
pub(crate) mod terminal;
#[path = "peer_workspace.rs"]
mod workspace;
#[derive(Default)]
pub(crate) struct PeerUi {
    pub views: HashMap<(AuthenticatedPeerId, m::Id), Entity<terminal::PeerTerminalView>>,
    pub forms: HashMap<AuthenticatedPeerId, (Entity<Field>, Entity<Field>)>,
    polls: HashMap<AuthenticatedPeerId, poll::PollSlot>,
    pub paused: HashSet<AuthenticatedPeerId>,
    pub attaching: HashSet<AuthenticatedPeerId>,
    pub pending: HashSet<AuthenticatedPeerId>,
    pub pages: HashMap<AuthenticatedPeerId, usize>,
    pub navigation: HashMap<AuthenticatedPeerId, u64>,
    pub recover: HashSet<(AuthenticatedPeerId, m::Id)>,
    pub frozen: bool,
    pub withdrawn: bool,
}
impl PeerUi {
    pub(crate) fn polls_drained(&self) -> bool {
        self.polls.is_empty()
    }
}
impl Root {
    pub(crate) fn select_local_source(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.peers.choose(source::Source::Local) {
            self.error = Some(error.to_string());
            return;
        }
        for view in self.peer_ui.views.values() {
            view.update(cx, |view, _| view.visible(false));
        }
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
            let _ = root.update_in(cx, |root, _, cx| {
                root.peers.inventory_pending = false;
                if let Ok(shellfox_service::native::NativeResponse::Remote(state)) = reply {
                    root.peers.inventory(&state.peers);
                    cx.notify();
                }
            });
        })
        .detach();
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
        for view in self.peer_ui.views.values() {
            view.update(cx, |view, _| view.visible(false));
        }
        self.settings = false;
        if let Err(error) = self.peers.choose(source::Source::Peer(peer.clone())) {
            self.error = Some(error.to_string());
            return;
        }
        self.peer_ui
            .forms
            .entry(peer.clone())
            .or_insert_with(|| (Field::new("", cx), Field::new("", cx)));
        if self.peers.peers.get(&peer).is_some_and(|state| {
            state.phase == source::Phase::Connected && state.snapshot.is_some()
        }) {
            self.focus_peer(window, cx);
            cx.notify();
            return;
        }
        self.connect_peer(peer, false, window, cx);
    }
    pub(crate) fn connect_peer(
        &mut self,
        peer: AuthenticatedPeerId,
        reconnect: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = self.host.clone() else {
            return;
        };
        if self.peer_ui.frozen {
            if !reconnect || !self.peer_ui.withdrawn || self.closing_window {
                return;
            }
            self.peer_ui.frozen = false;
            self.peer_ui.withdrawn = false;
        }
        if reconnect {
            self.retire_peer_views(&peer, cx);
            if let Err(error) = host
                .peer_existing(&peer)
                .and_then(|client| client.reconnect())
            {
                self.error = Some(error.to_string());
            }
        }
        let cycle = match self.peers.begin(&peer) {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        cx.spawn_in(window, async move |root, cx| {
            let result = async {
                let connection = host.peer_connection(peer)?;
                let origin = connection.connect().await?;
                let snapshot = connection.snapshot(&origin).await?;
                let home = connection
                    .execute(
                        &origin,
                        &m::GetHomeDirectory(shellfox_protocol::GetHomeDirectory {}),
                    )
                    .await
                    .ok();
                anyhow::Ok((origin, snapshot, home))
            }
            .await;
            let _ = root.update_in(cx, |root, _, cx| {
                if !root.peers.current(&cycle) || root.closing_window {
                    return;
                }
                root.peer_ui.paused.remove(&cycle.peer);
                match result {
                    Ok((origin, snapshot, home)) => {
                        root.peers.phase(&cycle, source::Phase::Connected, None);
                        if let Some(state) = root.peers.peers.get_mut(&cycle.peer) {
                            state.origin = Some(origin.witness);
                        }
                        if let Err(error) = root.peers.catalog(&cycle, snapshot) {
                            root.error = Some(error.to_string());
                        }
                        if let (Some(home), Some((path, _))) =
                            (home, root.peer_ui.forms.get(&cycle.peer))
                        {
                            path.update(cx, |field, cx| {
                                if field.text.is_empty() {
                                    field.set_text(home.cwd, cx);
                                }
                            });
                        }
                    }
                    Err(error) => {
                        let phase = host
                            .peer_existing(&cycle.peer)
                            .map(|peer| match peer.status() {
                                shellfox_client::manager_v2::Status::AwaitingApproval => {
                                    source::Phase::AwaitingApproval
                                }
                                shellfox_client::manager_v2::Status::Rejected => {
                                    source::Phase::Rejected
                                }
                                _ => source::Phase::Reconnecting,
                            })
                            .unwrap_or(source::Phase::Disconnected);
                        root.peers.phase(&cycle, phase, Some(error.to_string()));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
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
        self.peer_ui.polls.remove(peer);
        self.peer_ui.attaching.remove(peer);
        self.peer_ui.pending.remove(peer);
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
    fn focus_peer(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((peer, state)) = self.peers.selected_peer()
            && let Some(tab) = state.selected_tab
            && let Some(view) = self.peer_ui.views.get(&(peer.clone(), tab))
        {
            view.update(cx, |view, cx| {
                view.visible(true);
                view.focus(window);
                cx.notify();
            });
        }
    }
}
