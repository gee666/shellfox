use super::*;
impl Manager {
    pub(super) async fn apply(&mut self, event: Event) {
        match event {
            Event::Health { epoch, id, result } if epoch == self.epoch => {
                self.health_busy = false;
                if id.is_some_and(|id| !self.state.attempts.iter().any(|a| a.id == id)) {
                    return;
                }
                match result {
                    Ok((status, peers)) => {
                        if peers.len() > 1000 {
                            self.state.error =
                                Some(error(ErrorCode::Validation, "Too many discovered peers."));
                            self.state.status = RemoteNetworkStatus::Error;
                            return;
                        }
                        if status.login_url.as_ref().is_some_and(|url| {
                            url.len() > 2048
                                || !url.starts_with("https://login.tailscale.com/")
                                || url.chars().any(|c| c.is_control() || c.is_whitespace())
                        }) {
                            self.state.error =
                                Some(error(ErrorCode::Validation, "Invalid helper login URL."));
                            self.state.status = RemoteNetworkStatus::Error;
                            self.state.login_url = None;
                            return;
                        }
                        let ids: std::collections::HashSet<_> =
                            peers.iter().map(|peer| peer.id.clone()).collect();
                        self.state.peers.retain(|peer| {
                            peer.remembered
                                || self.connections.contains_key(&peer.id)
                                || ids.contains(&peer.id)
                        });
                        self.discovery
                            .retain(|id, _| self.state.peers.iter().any(|peer| &peer.id == id));
                        self.state.status = match status.status {
                            NetworkState::Disabled => RemoteNetworkStatus::Disabled,
                            NetworkState::Starting => RemoteNetworkStatus::Starting,
                            NetworkState::LoginRequired => RemoteNetworkStatus::LoginRequired,
                            NetworkState::Online => RemoteNetworkStatus::Online,
                            NetworkState::Error => RemoteNetworkStatus::Error,
                        };
                        self.state.login_url = if self
                            .state
                            .attempts
                            .iter()
                            .any(|a| matches!(a.kind, AttemptKind::Login))
                            && status.status == NetworkState::LoginRequired
                        {
                            status.login_url.or_else(|| self.state.login_url.clone())
                        } else {
                            None
                        };
                        self.state.error = status.error.map(|message| {
                            error(ErrorCode::NativeUnavailable, health::message(&message))
                        });
                        for peer in &mut self.state.peers {
                            peer.online = false;
                        }
                        for peer in peers {
                            if let Some(current) =
                                self.state.peers.iter_mut().find(|p| p.id == peer.id)
                            {
                                current.hostname = peer.hostname;
                                current.addresses = peer.addresses;
                                current.online = peer.online;
                            } else {
                                self.state.peers.push(RemotePeer {
                                    id: peer.id,
                                    hostname: peer.hostname,
                                    addresses: peer.addresses,
                                    online: peer.online,
                                    remembered: false,
                                    host_confirmed: false,
                                    connection: RemoteConnection::Disconnected,
                                    last_error: None,
                                    attempt_id: None,
                                });
                            }
                        }
                        if self.state.status == RemoteNetworkStatus::Online {
                            self.state
                                .attempts
                                .retain(|a| !matches!(a.kind, AttemptKind::Login));
                            self.state.login_url = None;
                            self.discover();
                            self.recover_established_peers();
                        }
                    }
                    Err(error) => {
                        self.state.error = Some(error);
                        self.state.status = RemoteNetworkStatus::Error;
                    }
                }
            }
            Event::Paired {
                epoch,
                id,
                peer,
                result,
                activation,
            } if epoch == self.epoch => {
                let Some(connection) = self.connections.get_mut(&peer)
                    .filter(|c| c.id == id && !c.cancel.is_cancelled()) else { return };
                if let Some(target) = self.state.peers.iter_mut().find(|p| p.id == peer) {
                    let result = result.and_then(|()| {
                        activation.activate(
                            self.stop.is_cancelled() || self.lost.is_cancelled(),
                            std::time::Instant::now(),
                        )
                    });
                    match result {
                        Ok(()) => {
                            connection.established = true;
                            target.connection = RemoteConnection::Connected;
                            target.host_confirmed = true;
                            target.last_error = None;
                            target.attempt_id = None;
                            self.state.attempts.retain(|a| a.id != id);
                        }
                        Err(error) => {
                            target.connection = match error.code {
                                ErrorCode::ApprovalRequired if !connection.established => RemoteConnection::AwaitingApproval,
                                ErrorCode::AccessDenied | ErrorCode::ApprovalRequired => RemoteConnection::Rejected,
                                ErrorCode::NativeUnavailable if connection.established => RemoteConnection::Reconnecting,
                                _ => RemoteConnection::Disconnected,
                            };
                            if matches!(target.connection, RemoteConnection::Rejected) {
                                target.attempt_id = None;
                                self.state.attempts.retain(|a| a.id != id);
                            }
                            target.last_error = Some(error);
                        }
                    }
                }
            }
            Event::PeerFailed {
                epoch,
                id,
                peer,
                error,
            } if epoch == self.epoch && self.connections.get(&peer).is_some_and(|c| c.id == id) => {
                self.cancel_outgoing_peer(&peer);
                if let Some(target) = self.state.peers.iter_mut().find(|p| p.id == peer) {
                    target.connection = match error.code {
                        ErrorCode::AccessDenied | ErrorCode::ApprovalRequired => RemoteConnection::Rejected,
                        ErrorCode::NativeUnavailable | ErrorCode::TargetLost => RemoteConnection::Reconnecting,
                        _ => RemoteConnection::Disconnected,
                    };
                    target.last_error = Some(error);
                }
            }
            Event::Discovered { epoch, peers } if epoch == self.epoch => {
                self.probe_busy = false;
                for (id, confirmed) in peers {
                    self.discovery.insert(id.clone(), std::time::Instant::now());
                    if let Some(peer) = self.state.peers.iter_mut().find(|peer| peer.id == id) {
                        peer.host_confirmed = confirmed;
                    }
                }
            }
            _ => {}
        }
    }
    pub(super) fn health(&mut self, id: Option<Uuid>, key: Option<Option<AuthKey>>) {
        let Some(network) = self.network.clone() else {
            return;
        };
        if self.health_busy {
            return;
        }
        self.health_busy = true;
        let epoch = self.epoch;
        self.spawn_job(async move {
            let result = async {
                let status = if let Some(key) = key {
                    network.login(key).await?
                } else {
                    network.status().await?
                };
                let peers = if status.status == NetworkState::Online {
                    network.peers().await?
                } else {
                    vec![]
                };
                Ok((status, peers))
            }
            .await;
            Event::Health { epoch, id, result }
        });
    }
}
