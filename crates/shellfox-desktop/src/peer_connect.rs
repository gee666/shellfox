use super::*;
use crate::host_client::peer_v1::V1Binding;
use shellfox_client::ConnectionStatus;
use shellfox_protocol as wire;

struct Connected {
    snapshot: wire::ManagerSnapshot,
    home: Option<String>,
}

async fn connect(binding: &V1Binding) -> anyhow::Result<Connected> {
    binding.connect().await?;
    let snapshot = binding.v1_snapshot().await?;
    let home = binding
        .v1_execute(&wire::GetHomeDirectory {})
        .await
        .ok()
        .map(|home| home.cwd);
    Ok(Connected { snapshot, home })
}

impl Root {
    pub(super) fn connect_catalog(
        &mut self,
        peer: AuthenticatedPeerId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = self.host.clone() else {
            return;
        };
        if self.peer_ui.frozen || self.closing_window {
            return;
        }
        let binding = match self.legacy_peer_binding(&peer) {
            Some(binding) => Ok(binding),
            None => host.shared_peer_v1(&peer),
        };
        let binding = match binding {
            Ok(binding) => binding,
            Err(error) => {
                self.error = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        let retry_rejected = self
            .peers
            .peers
            .get(&peer)
            .is_some_and(|state| state.phase == source::Phase::Rejected);
        if retry_rejected {
            host.retry_rejected_v1(&peer);
            self.retire_peer_views(&peer, cx);
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
            let result = connect(&binding).await;
            let mut phase = match binding.state().status {
                ConnectionStatus::AwaitingApproval => source::Phase::AwaitingApproval,
                ConnectionStatus::Rejected => source::Phase::Rejected,
                _ => source::Phase::Reconnecting,
            };
            let mut keep = true;
            if phase == source::Phase::AwaitingApproval {
                // Approval was lost between the owner gate and opening this view.
                // Withdraw the secondary view; only owner attempts wait for approval.
                let _ = host.disconnect_peer(&peer).await;
                keep = false;
                phase = source::Phase::Rejected;
            }
            let _ = root.update_in(cx, |root, window, cx| {
                if !root.peers.current(&cycle) || root.closing_window {
                    return;
                }
                root.peer_ui.paused.remove(&cycle.peer);
                if keep {
                    root.peer_ui.legacy.insert(peer.clone(), binding);
                }
                match result {
                    Ok(Connected { snapshot, home }) => {
                        root.peers.phase(&cycle, source::Phase::Connected, None);
                        if let Some(state) = root.peers.peers.get_mut(&cycle.peer) {
                            state.home = home;
                        }
                        match root.peers.catalog(&cycle, snapshot) {
                            Ok(true) => {
                                root.reconcile_peer_file_catalog(&peer, cx);
                                root.resume_peer_selection(&peer, window, cx);
                            }
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
                        if phase == source::Phase::Reconnecting {
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
