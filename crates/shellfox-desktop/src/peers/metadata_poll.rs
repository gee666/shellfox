//! Peer catalog reads after connect: again only when the host reports a change.
use super::*;
use std::time::{Duration, Instant};

impl Root {
    pub(crate) fn poll_peer_sources(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.peer_ui.frozen || self.close.closing {
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
                    && state.owner_connected
                    && state.phase == source::Phase::Reconnecting
                    && !state.pending
                    && due)
                    .then(|| id.clone())
            })
            .collect();
        for id in retries {
            self.peer_ui
                .metadata_polls
                .insert(id.clone(), Some(now + Duration::from_secs(2)));
            self.notify_at(now + Duration::from_secs(2), cx);
            self.connect_catalog(id, window, cx);
        }
        // A stream outlives its connection only while a live view still needs it to recover.
        let idle: Vec<_> = self.peer_ui.v1_streams.keys()
            .filter(|peer| {
                self.peers.peers.get(*peer).is_none_or(|state| state.phase != source::Phase::Connected)
                    && !self.peer_ui.v1_views.iter()
                        .any(|((id, _), view)| id == *peer && !view.read(cx).peer_released())
            })
            .cloned()
            .collect();
        for peer in idle {
            self.peer_ui.v1_streams.remove(&peer);
        }
        let peers: Vec<_> = self
            .peers
            .peers
            .iter()
            .filter_map(|(peer, state)| {
                if state.phase != source::Phase::Connected
                    || self.peer_ui.pending.contains(peer)
                    || state.pending
                    || matches!(self.peer_ui.metadata_polls.get(peer), Some(None))
                {
                    return None;
                }
                let cycle = source::PeerCycle {
                    peer: peer.clone(),
                    generation: state.cycle,
                };
                let api = self.peer_api(&cycle).ok()?;
                let catalog = api.binding().state().catalog;
                let current = CatalogRead::covers(self.peer_ui.catalog_reads.get(peer), cycle.generation, catalog);
                Some((cycle, api, catalog, current))
            })
            .collect();
        for (cycle, api, catalog, current) in peers {
            // The stream's polls bump the catalog on every host change.
            self.start_v1_stream(cycle.peer.clone(), api.binding().clone(), window, cx);
            if current {
                continue;
            }
            self.peer_ui.metadata_polls.insert(cycle.peer.clone(), None);
            cx.spawn_in(window, async move |root, cx| {
                let result = api.snapshot().await;
                let _ = root.update_in(cx, |root, _, cx| {
                    root.peer_ui.metadata_polls.remove(&cycle.peer);
                    if !root.peers.current(&cycle) {
                        return;
                    }
                    if root.peer_ui.frozen || root.close.closing {
                        return;
                    }
                    match result {
                        Ok(snapshot) => {
                            root.peer_ui.catalog_reads.insert(
                                cycle.peer.clone(),
                                CatalogRead::new(cycle.generation, catalog),
                            );
                            match root.peers.catalog(&cycle, snapshot) {
                                Ok(true) => root.reconcile_peer_file_catalog(&cycle.peer, cx),
                                Ok(false) => {}
                                Err(error) => root.notify_machine_error(&error.to_string(), cx),
                            }
                        }
                        Err(error) => {
                            root.notify_machine_error(&error.to_string(), cx);
                            // Reconnecting retries on its own schedule.
                            let retry = Instant::now() + Duration::from_secs(1);
                            root.peer_ui.metadata_polls.insert(cycle.peer.clone(), Some(retry));
                            root.notify_at(retry, cx);
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

/// The catalog revision a connected peer's last read saw.
pub(super) struct CatalogRead {
    generation: u64,
    catalog: u64,
}

impl CatalogRead {
    pub(super) fn new(generation: u64, catalog: u64) -> Self {
        Self { generation, catalog }
    }

    /// A connected peer is read once per connection, then again only after a reported change.
    fn covers(read: Option<&Self>, generation: u64, catalog: u64) -> bool {
        read.is_some_and(|read| read.generation == generation && read.catalog == catalog)
    }
}

#[cfg(test)]
mod tests {
    use super::CatalogRead;

    #[test]
    fn an_idle_connected_peer_is_read_once() {
        assert!(!CatalogRead::covers(None, 3, 0), "the first read after connect");
        let read = CatalogRead { generation: 3, catalog: 0 };
        assert!(CatalogRead::covers(Some(&read), 3, 0), "no change, no read");
        assert!(!CatalogRead::covers(Some(&read), 3, 1), "a reported change reads again");
        assert!(!CatalogRead::covers(Some(&read), 4, 0), "a new connection reads again");
    }
}
