use super::*;
use crate::host_client::{peer::LegacyPeerRequired, peer_v1::V1Binding};
use shellfox_client::{
    ConnectionStatus,
    manager_v2::{FallbackPolicy, ManagerBindingWitness, Status},
};
use shellfox_protocol as wire;

struct Connected {
    snapshot: m::Snapshot,
    witness: Option<ManagerBindingWitness>,
    home: Option<String>,
}

async fn connect_legacy(binding: &V1Binding) -> anyhow::Result<Connected> {
    binding.connect().await?;
    let snapshot = catalog_v1::project(binding.v1_snapshot().await?)?;
    let home = binding
        .v1_execute(&wire::GetHomeDirectory {})
        .await
        .ok()
        .map(|home| home.cwd);
    Ok(Connected {
        snapshot,
        witness: None,
        home,
    })
}

impl Root {
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
        let mut legacy = self.legacy_peer_binding(&peer);
        let retry_rejected = self
            .peers
            .peers
            .get(&peer)
            .is_some_and(|state| state.phase == source::Phase::Rejected);
        if reconnect || retry_rejected {
            host.retry_rejected_v1(&peer);
            self.retire_peer_views(&peer, cx);
            if legacy.is_none()
                && let Ok(connection) = host.peer_existing(&peer)
                && let Err(error) = connection.reconnect()
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
            let result = match &legacy {
                Some(binding) => connect_legacy(binding).await,
                None => {
                    let result = async {
                        let connection = host.peer_connection(peer.clone())?;
                        let origin = connection
                            .connect_with_policy(FallbackPolicy::OfferLegacy)
                            .await?;
                        let snapshot = connection.snapshot(&origin).await?;
                        let home = connection
                            .execute(&origin, &m::GetHomeDirectory(wire::GetHomeDirectory {}))
                            .await
                            .ok()
                            .map(|home| home.cwd);
                        anyhow::Ok(Connected {
                            snapshot,
                            witness: Some(origin.witness),
                            home,
                        })
                    }
                    .await;
                    match result {
                        Err(error) if error.is::<LegacyPeerRequired>() => {
                            match host.shared_peer_v1(&peer) {
                                Ok(binding) => {
                                    let result = connect_legacy(&binding).await;
                                    legacy = Some(binding);
                                    result
                                }
                                Err(error) => Err(error),
                            }
                        }
                        result => result,
                    }
                }
            };
            let phase = legacy
                .as_ref()
                .map(|binding| match binding.state().status {
                    ConnectionStatus::AwaitingApproval => source::Phase::AwaitingApproval,
                    ConnectionStatus::Rejected => source::Phase::Rejected,
                    _ => source::Phase::Reconnecting,
                })
                .unwrap_or_else(|| {
                    host.peer_existing(&peer)
                        .map_or(source::Phase::Disconnected, |connection| {
                            match connection.status() {
                                Status::AwaitingApproval => source::Phase::AwaitingApproval,
                                Status::Rejected => source::Phase::Rejected,
                                _ => source::Phase::Reconnecting,
                            }
                        })
                });
            let _ = root.update_in(cx, |root, window, cx| {
                if !root.peers.current(&cycle) || root.closing_window {
                    return;
                }
                root.peer_ui.paused.remove(&cycle.peer);
                if let Some(binding) = legacy {
                    root.peer_ui.legacy.insert(peer.clone(), binding);
                }
                match result {
                    Ok(Connected {
                        witness,
                        snapshot,
                        home,
                    }) => {
                        root.peers.phase(&cycle, source::Phase::Connected, None);
                        if let Some(state) = root.peers.peers.get_mut(&cycle.peer) {
                            state.origin = witness;
                            state.home = home;
                        }
                        match root.peers.catalog(&cycle, snapshot) {
                            Ok(true) => root.resume_peer_selection(&peer, window, cx),
                            Ok(false) => return,
                            Err(error) => {
                                root.error = Some(error.to_string());
                                root.peers.phase(
                                    &cycle,
                                    source::Phase::Reconnecting,
                                    Some(connection_view::CONNECTION_FAILURE.into()),
                                );
                                cx.notify();
                                return;
                            }
                        }
                        if root.peer_ui.pending_create.as_ref() == Some(&peer)
                            && root.peers.selected == source::Source::Peer(peer.clone())
                        {
                            root.peer_ui.pending_create = None;
                            root.open_peer_directory(window, cx);
                        }
                    }
                    Err(error) => {
                        let detail = if phase == source::Phase::Reconnecting {
                            connection_view::CONNECTION_FAILURE.into()
                        } else {
                            error.to_string()
                        };
                        root.peers.phase(&cycle, phase, Some(detail));
                        #[cfg(not(feature = "experimental-shellfox-native"))]
                        if matches!(
                            phase,
                            source::Phase::AwaitingApproval | source::Phase::Reconnecting
                        ) {
                            root.peer_ui.metadata_polls.insert(
                                peer.clone(),
                                Some(std::time::Instant::now() + std::time::Duration::from_secs(2)),
                            );
                        }
                        if phase != source::Phase::AwaitingApproval
                            && root.peer_ui.pending_create.as_ref() == Some(&peer)
                        {
                            root.peer_ui.pending_create = None;
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}
