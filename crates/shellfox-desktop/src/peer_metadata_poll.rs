//! Stable peer catalog refresh without a Native0 terminal codec.
use super::*;
use std::time::{Duration, Instant};

impl Root {
    pub(crate) fn poll_peer_sources(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.peer_ui.frozen || self.closing_window {
            return;
        }
        let now = Instant::now();
        let retries: Vec<_> = self
            .peers
            .peers
            .iter()
            .filter_map(|(id, state)| {
                let due = self
                    .peer_ui
                    .metadata_polls
                    .get(id)
                    .is_some_and(|deadline| deadline.is_some_and(|time| time <= now));
                (state.remembered
                    && state.online
                    && matches!(
                        state.phase,
                        source::Phase::Reconnecting | source::Phase::AwaitingApproval
                    )
                    && !state.pending
                    && due)
                    .then(|| (id.clone(), state.phase))
            })
            .collect();
        for (id, phase) in retries {
            self.peer_ui
                .metadata_polls
                .insert(id.clone(), Some(now + Duration::from_secs(2)));
            if phase == source::Phase::Reconnecting
                && self.legacy_peer_binding(&id).is_none()
                && let Some(host) = &self.host
                && let Ok(connection) = host.peer_existing(&id)
                && let Err(error) = connection.reconnect()
            {
                self.error = Some(error.to_string());
                continue;
            }
            self.connect_peer(id, false, window, cx);
        }
        let peers: Vec<_> = self
            .peers
            .peers
            .iter()
            .filter_map(|(peer, state)| {
                if state.phase != source::Phase::Connected
                    || self.peer_ui.pending.contains(peer)
                    || state.pending
                    || self
                        .peer_ui
                        .metadata_polls
                        .get(peer)
                        .is_some_and(|deadline| deadline.is_none_or(|time| time > now))
                {
                    return None;
                }
                let cycle = source::PeerCycle {
                    peer: peer.clone(),
                    generation: state.cycle,
                };
                let api = self.peer_api(&cycle).ok()?;
                Some((cycle, api))
            })
            .collect();
        for (cycle, api) in peers {
            self.peer_ui.metadata_polls.insert(cycle.peer.clone(), None);
            cx.spawn_in(window, async move |root, cx| {
                let result = api.snapshot().await;
                let _ = root.update_in(cx, |root, _, cx| {
                    if !root.peers.current(&cycle) {
                        return;
                    }
                    root.peer_ui.metadata_polls.insert(
                        cycle.peer.clone(),
                        Some(Instant::now() + Duration::from_secs(1)),
                    );
                    if root.peer_ui.frozen || root.closing_window {
                        return;
                    }
                    match result {
                        Ok(snapshot) => {
                            if let Err(error) = root.peers.catalog(&cycle, snapshot) {
                                root.notify_machine_error(&error.to_string(), cx);
                            }
                        }
                        Err(error) => {
                            root.notify_machine_error(&error.to_string(), cx);
                            root.peers.phase(
                                &cycle,
                                source::Phase::Reconnecting,
                                Some(connection_view::CONNECTION_FAILURE.into()),
                            );
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
    }
}
