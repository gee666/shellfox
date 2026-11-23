use super::*;
use crate::settings::{native_services::NativeServices, native_state, pairing::Watch};
use shellfox_protocol::CancellationToken;
use shellfox_service::{
    native::{NativeAction, NativeResponse},
    remote::native::{ConnectResult, RemoteControlState, RemoteRequest},
};
use std::time::Instant;
use uuid::Uuid;

impl Root {
    pub(super) fn accept_peer_remote(
        &mut self,
        state: RemoteControlState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.peer_ui.inventory_generation += 1;
        self.peers.inventory(&state.peers);
        if self.peer_ui.pending_create.as_ref().is_some_and(|id| {
            self.peers.peers.get(id).is_none_or(source::PeerState::creation_connection_failed)
        }) {
            self.peer_ui.pending_create = None;
        }
        self.peer_ui
            .sidebar_layout
            .borrow_mut()
            .retain(|peer, _| self.peers.peers.contains_key(peer));
        self.warm_connected_catalogs(&state, window, cx);
        self.update_pending_access(
            state
                .pending
                .iter()
                .map(|pending| (pending.peer.id.as_str(), pending.peer.hostname.as_str())),
            cx,
        );
        cx.notify();
    }

    pub(crate) fn connect_peer(
        &mut self,
        peer: AuthenticatedPeerId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.peer_ui.frozen || self.close.closing {
            return;
        }
        let Some(state) = self.peers.peers.get(&peer) else {
            return;
        };
        if state.pending {
            return;
        }
        if state.owner_connected {
            self.connect_catalog(peer, window, cx);
            return;
        }
        if !matches!(
            state.connection_action(),
            Some(source::ConnectionAction::Connect | source::ConnectionAction::Retry)
        ) {
            return;
        }
        let Some(host) = self.host.clone() else {
            return;
        };
        let cycle = match self.peers.begin(&peer) {
            Ok(cycle) => cycle,
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        let started = Instant::now();
        cx.spawn_in(window, async move |root, cx| {
            let response = host
                .execute(NativeAction::Remote(RemoteRequest::Connect {
                    peer_id: peer.as_str().into(),
                }))
                .await;
            let _ = root.update_in(cx, |root, window, cx| {
                if !root.peers.current(&cycle) || root.close.closing {
                    return;
                }
                root.peers.phase(&cycle, source::Phase::Disconnected, None);
                match response {
                    Ok(NativeResponse::RemoteConnect { state, result }) => {
                        root.watch_peer_pair(&cycle, &state, result, started, window, cx);
                        root.accept_peer_remote(state, window, cx);
                    }
                    Ok(NativeResponse::Remote(state)) => root.accept_peer_remote(state, window, cx),
                    Err(error) => root.pair_request_failed(error.to_string(), &peer, window, cx),
                    Ok(_) => root.pair_request_failed(
                        "Unexpected Connect reply.".into(),
                        &peer,
                        window,
                        cx,
                    ),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn cancel_peer_pair(
        &mut self,
        peer: AuthenticatedPeerId,
        attempt_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.peer_ui.frozen || self.close.closing {
            return;
        }
        let Some(state) = self.peers.peers.get_mut(&peer) else {
            return;
        };
        if state.connection_action() != Some(source::ConnectionAction::Cancel(attempt_id)) {
            self.poll_peer_inventory(window, cx);
            return;
        }
        let Some(host) = self.host.clone() else {
            return;
        };
        state.pending = true;
        let cycle = source::PeerCycle {
            peer: peer.clone(),
            generation: state.cycle,
        };
        if let Some((_, lifetime)) = self.peer_ui.pair_watches.remove(&peer) {
            lifetime.cancel();
        }
        if self.peer_ui.pending_create.as_ref() == Some(&peer) {
            self.peer_ui.pending_create = None;
        }
        cx.spawn_in(window, async move |root, cx| {
            let response = host
                .execute(NativeAction::Remote(RemoteRequest::Cancel { attempt_id }))
                .await;
            let _ = root.update_in(cx, |root, window, cx| {
                let Some(state) = root
                    .peers
                    .peers
                    .get_mut(&peer)
                    .filter(|state| state.cycle == cycle.generation)
                else {
                    return;
                };
                state.pending = false;
                match response {
                    Ok(NativeResponse::Remote(state)) => root.accept_peer_remote(state, window, cx),
                    Err(error) => root.pair_request_failed(error.to_string(), &peer, window, cx),
                    Ok(_) => root.pair_request_failed(
                        "Unexpected Cancel reply.".into(),
                        &peer,
                        window,
                        cx,
                    ),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn pair_request_failed(
        &mut self,
        message: String,
        peer: &AuthenticatedPeerId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.peer_ui.pending_create.as_ref() == Some(peer) {
            self.peer_ui.pending_create = None;
        }
        if let Some(state) = self.peers.peers.get_mut(peer) {
            state.error = Some(message);
        }
        self.poll_peer_inventory(window, cx);
    }

    fn watch_peer_pair(
        &mut self,
        cycle: &source::PeerCycle,
        state: &RemoteControlState,
        result: ConnectResult,
        started: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ConnectResult::Created(id) = result else {
            return;
        };
        let Some(host) = self.host.clone() else {
            return;
        };
        let lifetime = CancellationToken::new();
        let Some(watch) = Watch::new(
            &native_state::project(state.clone()),
            cycle.peer.as_str().into(),
            result,
            started,
            lifetime.clone(),
        ) else {
            return;
        };
        let cycle = cycle.clone();
        if let Some((_, old)) = self
            .peer_ui
            .pair_watches
            .insert(cycle.peer.clone(), (id, lifetime))
        {
            old.cancel();
        }
        cx.spawn_in(window, async move |root, cx| {
            let executor = cx.background_executor().clone();
            let services = NativeServices::new(host);
            let result = watch
                .run(
                    &services,
                    |delay| executor.timer(delay),
                    |permit| {
                        root.update_in(cx, |root, _, _| {
                            (permit.active_at(Instant::now())
                                && !root.peer_ui.frozen
                                && !root.close.closing
                                && root.peers.current(&cycle)
                                && root.peers.peers[&cycle.peer].owner_pair_attempt == Some(id)
                                && !root.peers.peers[&cycle.peer].pending)
                                .then_some(())
                        })
                        .ok()
                        .flatten()
                    },
                )
                .await;
            let _ = root.update_in(cx, |root, window, cx| {
                if root
                    .peer_ui
                    .pair_watches
                    .get(&cycle.peer)
                    .map(|(current, _)| *current)
                    != Some(id)
                {
                    return;
                }
                root.peer_ui.pair_watches.remove(&cycle.peer);
                if result.is_err()
                    && let Some(state) = root
                        .peers
                        .peers
                        .get_mut(&cycle.peer)
                        .filter(|state| state.owner_pair_attempt == Some(id))
                {
                    state.stopped_pair_attempt = Some(id);
                }
                root.poll_peer_inventory(window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn stop_peer_pairing(&mut self) {
        for (_, (_, lifetime)) in self.peer_ui.pair_watches.drain() {
            lifetime.cancel();
        }
    }
}
