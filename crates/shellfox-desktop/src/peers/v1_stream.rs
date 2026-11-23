use super::*;
use crate::{host_client::peer_v1::V1Binding, terminal::TerminalView};
use shellfox_client::ConnectionStatus;
use shellfox_protocol::{HostEvent, TerminalEvent};
use std::{sync::Arc, time::Duration};

impl Root {
    pub(crate) fn peer_tab_busy(&self, peer: &AuthenticatedPeerId, tab: TabId) -> Option<bool> {
        self.peer_ui.v1_activity.get(&(peer.clone(), tab)).copied()
    }

    /// The selected peer tab has no view and its last open didn't fail.
    pub(crate) fn peer_attach_due(&self) -> bool {
        if self.is_local_source()
            || self.peer_ui.frozen
            || self.close.closing
            || self.host.is_none()
        {
            return false;
        }
        let Some((peer, state)) = self.peers.selected_peer() else {
            return false;
        };
        let Some(tab) = state.selected_tab else {
            return false;
        };
        let key = (peer.clone(), tab);
        state.phase == source::Phase::Connected
            && !self.peer_ui.v1_views.contains_key(&key)
            && self.peer_ui.v1_failed.as_ref() != Some(&key)
    }

    pub(crate) fn open_v1_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.peer_ui.frozen || self.close.closing {
            return;
        }
        let Some((peer, state)) = self.peers.selected_peer() else {
            return;
        };
        let Some(tab) = state.selected_tab else {
            self.hide_v1_terminals(cx);
            return;
        };
        let key = (peer.clone(), tab);
        self.show_v1_terminal(Some(&key), cx);
        let Some((peer, state)) = self.peers.selected_peer() else {
            return;
        };
        if let Some(view) = self.peer_ui.v1_views.get(&key) {
            view.read(cx).focus.focus(window);
            return;
        }
        if state.phase != source::Phase::Connected {
            return;
        }
        if self.peer_ui.v1_views.len() >= 24 {
            self.peer_ui
                .v1_views
                .retain(|_, view| !view.read(cx).peer_released());
            if self.peer_ui.v1_views.len() >= 24 {
                self.error = Some("Waiting for previous peer terminal views to detach.".into());
                return;
            }
        }
        let Some(host) = &self.host else {
            return;
        };
        let legacy = self.legacy_peer_binding(peer);
        let frontend_replies = legacy.is_some();
        let binding = legacy.map_or_else(|| host.shared_peer_v1(peer), Ok);
        let result = binding.and_then(|binding| {
            let view = TerminalView::open_peer(binding.clone(), tab, frontend_replies, window, cx)?;
            Ok((binding, view))
        });
        match result {
            Ok((binding, view)) => {
                self.start_v1_stream(key.0.clone(), binding, window, cx);
                view.update(cx, |view, _| view.enable_file_links());
                let link_key = key.clone();
                cx.subscribe_in(
                    &view,
                    window,
                    move |root, emitter, event: &crate::terminal::TerminalFileLink, window, cx| {
                        if root.peer_ui.v1_views.get(&link_key) == Some(emitter) {
                            root.open_peer_terminal_file_link(
                                &link_key.0,
                                link_key.1,
                                event,
                                window,
                                cx,
                            );
                        }
                    },
                )
                .detach();
                self.peer_ui.v1_views.insert(key, view);
                self.apply_terminal_preferences(window, cx);
            }
            Err(error) => {
                self.error = Some(error.to_string());
                self.peer_ui.v1_failed = Some(key);
            }
        }
        cx.notify();
    }

    pub(super) fn start_v1_stream(
        &mut self,
        peer: AuthenticatedPeerId,
        binding: Arc<V1Binding>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let wake = self.peer_ui.stream_wake.clone();
        self.peer_ui
            .v1_streams
            .entry(peer.clone())
            .or_insert_with(|| {
                cx.spawn_in(window, async move |root, cx| {
                    let mut retry = Duration::ZERO;
                    let mut changes = binding.state_changes();
                    let mut catalog = binding.state().catalog;
                    loop {
                        if !retry.is_zero() {
                            cx.background_executor().timer(retry).await;
                            retry = Duration::ZERO;
                        }
                        let unfrozen = wake.notified();
                        let Ok((frozen, attaching)) = root.update_in(cx, |root, _, cx| {
                            let attaching = root.peer_ui.v1_views.iter().any(|((id, _), view)| {
                                id == &peer && view.read(cx).peer_attaching()
                            });
                            (root.peer_ui.frozen, attaching)
                        }) else {
                            break;
                        };
                        if frozen {
                            unfrozen.await;
                            continue;
                        }
                        // A view's recovery can reset the wire sequence even when control
                        // has not changed, so no batch is polled through that cut. The
                        // attach reply arrives on the view, not here; it is short.
                        if attaching {
                            retry = Duration::from_millis(30);
                            continue;
                        }
                        if binding.state().status == ConnectionStatus::Rejected {
                            if changes.changed().await.is_err() {
                                break;
                            }
                            continue;
                        }
                        if binding.connect().await.is_ok() {
                            let connection = binding.state().connection_id;
                            if let Ok(poll) = binding.poll().await {
                                // The host long poll already waits for events,
                                // so a successful batch re-polls immediately.
                                if binding.state().connection_id != connection {
                                    continue;
                                }
                                // Any Changed event bumps the catalog; the sidebar re-reads it.
                                let changed =
                                    std::mem::replace(&mut catalog, binding.state().catalog)
                                        != catalog;
                                if root
                                    .update_in(cx, |root, _, cx| {
                                        root.deliver_v1_events(&peer, connection, poll.events, cx);
                                        if changed {
                                            cx.notify();
                                        }
                                    })
                                    .is_err()
                                {
                                    break;
                                }
                                continue;
                            }
                        }
                        let state = binding.state();
                        if root
                            .update_in(cx, |root, _, cx| {
                                if let Some(cycle) = root
                                    .peers
                                    .stream_failed(&peer, connection_view::CONNECTION_FAILURE)
                                {
                                    root.retry_peer_read(&cycle, cx);
                                }
                                let count = root.peer_ui.v1_activity.len();
                                root.peer_ui.v1_activity.retain(|(id, _), _| id != &peer);
                                if root.peer_ui.v1_activity.len() != count {
                                    cx.notify();
                                }
                                for ((id, _), view) in &root.peer_ui.v1_views {
                                    if id == &peer {
                                        view.update(cx, |_, cx| cx.notify());
                                    }
                                }
                            })
                            .is_err()
                        {
                            break;
                        }
                        retry = state.retry_after.unwrap_or(Duration::from_secs(1));
                    }
                })
            });
    }

    fn deliver_v1_events(
        &mut self,
        peer: &AuthenticatedPeerId,
        connection: u64,
        events: Vec<HostEvent>,
        cx: &mut Context<Self>,
    ) {
        let mut tabs = HashMap::<_, Vec<_>>::new();
        let mut activity_changed = false;
        let selected = self
            .peers
            .selected_peer()
            .and_then(|(id, state)| (id == peer).then_some(state.selected_tab).flatten());
        let known: HashSet<_> = self
            .peers
            .peers
            .get(peer)
            .and_then(|state| state.snapshot.as_ref())
            .into_iter()
            .flat_map(|snapshot| &snapshot.sessions)
            .flat_map(|session| session.tabs.iter().map(|tab| tab.id))
            .collect();
        self.peer_ui.v1_activity.retain(|(id, tab), _| {
            let keep = id != peer || known.contains(tab);
            activity_changed |= !keep;
            keep
        });
        for event in events {
            let HostEvent::Terminal { event } = event else {
                continue;
            };
            let tab = match &event {
                TerminalEvent::Data { tab_id, .. }
                | TerminalEvent::Settlement { tab_id, .. }
                | TerminalEvent::Exit { tab_id, .. }
                | TerminalEvent::Error { tab_id, .. }
                | TerminalEvent::Control { tab_id, .. }
                | TerminalEvent::Activity { tab_id, .. } => tab_id,
            };
            let tab = *tab;
            match &event {
                TerminalEvent::Activity { busy, .. } => {
                    let key = (peer.clone(), tab);
                    let detached =
                        self.peer_ui.v1_views.contains_key(&key) && selected != Some(tab);
                    if !detached && known.contains(&tab) {
                        activity_changed |=
                            self.peer_ui.v1_activity.insert(key, *busy) != Some(*busy);
                    }
                    continue;
                }
                TerminalEvent::Exit { .. } => {
                    activity_changed |= self
                        .peer_ui
                        .v1_activity
                        .remove(&(peer.clone(), tab))
                        .is_some();
                }
                _ => {}
            }
            tabs.entry(tab).or_default().push(event);
        }
        for ((id, tab), view) in &self.peer_ui.v1_views {
            if id != peer {
                continue;
            }
            if let Some(events) = tabs.remove(tab) {
                view.update(cx, |view, cx| view.peer_events(events, connection, cx));
            }
        }
        if activity_changed {
            cx.notify();
        }
    }

    fn show_v1_terminal(
        &mut self,
        key: Option<&(AuthenticatedPeerId, TabId)>,
        cx: &mut Context<Self>,
    ) {
        let mut changed = false;
        for (id, view) in &self.peer_ui.v1_views {
            let visible = Some(id) == key;
            if !visible {
                changed |= self.peer_ui.v1_activity.remove(id).is_some();
            }
            view.update(cx, |view, cx| view.set_peer_visible(visible, cx));
        }
        if changed {
            cx.notify();
        }
    }

    pub(super) fn hide_v1_terminals(&mut self, cx: &mut Context<Self>) {
        self.show_v1_terminal(None, cx);
    }

    pub(crate) fn retire_v1_terminals(&mut self, peer: &AuthenticatedPeerId) {
        self.peer_ui.v1_streams.remove(peer);
        self.peer_ui.v1_views.retain(|(id, _), _| id != peer);
        self.peer_ui.v1_activity.retain(|(id, _), _| id != peer);
    }
}
