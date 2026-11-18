use super::*;
use serde::{de::DeserializeOwned, Deserialize};
use wire::{Endpoint, Reply, Validate};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Discovery {
    service: String,
    protocol_version: u8,
}
impl Validate for Discovery {
    fn validate(&self) -> Result<(), wire::ValidationError> {
        wire::validation::ensure(self.service == "shellfox" && self.protocol_version == 1)
    }
}
pub(super) async fn send<T: DeserializeOwned + Validate>(
    network: &dyn RemoteNetwork,
    peer: &wire::AuthenticatedPeerId,
    endpoint: Endpoint,
    id: Uuid,
    cancel: CancellationToken,
) -> Result<Reply<T>, AppError> {
    let body = if endpoint == Endpoint::Discover {
        b"{}".to_vec()
    } else {
        serde_json::to_vec(&wire::ViewIdentity { client_id: id })
            .map_err(|_| error(ErrorCode::Internal, "Connection identity encoding failed."))?
    };
    let timeout = Duration::from_secs(if endpoint == Endpoint::Discover {
        3
    } else {
        25
    });
    let limit = if endpoint == Endpoint::Events {
        wire::MAX_RESPONSE_BYTES
    } else {
        4096
    };
    let send = network.send(
        peer,
        endpoint,
        body,
        wire::RequestLimits {
            deadline: std::time::Instant::now() + timeout,
            cancel: cancel.clone(),
            max_request_bytes: wire::MAX_REQUEST_BYTES,
            max_response_bytes: limit,
        },
    );
    let bytes = tokio::select! {
        _=cancel.cancelled()=>return Err(error(ErrorCode::NativeUnavailable,"Connection attempt cancelled.")),
        result=tokio::time::timeout(timeout,send)=>result.map_err(|_|error(ErrorCode::NativeUnavailable,"Remote request timed out. Reconnect explicitly."))?.map_err(|_|error(ErrorCode::NativeUnavailable,"Remote transport failed. Do not retry an uncertain mutation automatically."))?,
    };
    if bytes.len() > limit {
        return Err(error(
            ErrorCode::Validation,
            "Remote response exceeds its bound.",
        ));
    }
    wire::decode(&bytes).map_err(|_| {
        error(
            ErrorCode::Unsupported,
            "Remote host contract is unavailable or invalid.",
        )
    })
}
pub(super) fn success<T>(reply: Reply<T>) -> Result<T, AppError> {
    match reply {
        Reply::Success(value) => Ok(value),
        Reply::Failure(error) => Err(error),
    }
}
impl Manager {
    pub(super) fn connect(&mut self, peer_id: &str, resume: Option<Uuid>) -> Result<(), AppError> {
        let network = self
            .network
            .clone()
            .ok_or_else(|| error(ErrorCode::Unsupported, "Enable remote networking first."))?;
        if self.state.status != RemoteNetworkStatus::Online {
            return Err(error(
                ErrorCode::NativeUnavailable,
                "The transport is not online.",
            ));
        }
        if self.connections.len() >= 16 && !self.connections.contains_key(peer_id) {
            return Err(error(
                ErrorCode::Validation,
                "Remote connection limit reached.",
            ));
        }
        let target = self
            .state
            .peers
            .iter_mut()
            .find(|peer| peer.id == peer_id)
            .ok_or_else(|| error(ErrorCode::NotFound, "Peer has not been discovered."))?;
        if resume.is_none()
            && matches!(
                target.connection,
                RemoteConnection::Connecting
                    | RemoteConnection::Connected
                    | RemoteConnection::AwaitingApproval
            )
        {
            return Ok(());
        }
        if resume.is_some()
            && !matches!(
                target.connection,
                RemoteConnection::AwaitingApproval
                    | RemoteConnection::Reconnecting
                    | RemoteConnection::Disconnected
            )
        {
            return Err(error(
                ErrorCode::Validation,
                "This connection attempt cannot continue. Reconnect explicitly.",
            ));
        }
        let peer = wire::AuthenticatedPeerId::new(peer_id.to_owned())
            .map_err(|_| error(ErrorCode::Validation, "Invalid peer identity."))?;
        let id = resume.unwrap_or_else(Uuid::new_v4);
        let epoch = self.epoch;
        let cancel = self.cycle.child_token();
        if let Some(previous) = self.connections.remove(peer_id) {
            previous.cancel.cancel();
            if previous.id != id {
                self.state
                    .attempts
                    .retain(|attempt| attempt.id != previous.id);
            }
        }
        self.connections.insert(
            peer_id.into(),
            Connection {
                id,
                cancel: cancel.clone(),
            },
        );
        target.connection = if resume.is_some() {
            RemoteConnection::Reconnecting
        } else {
            RemoteConnection::Connecting
        };
        target.attempt_id = Some(id);
        target.last_error = None;
        target.remembered = true;
        if !self.state.attempts.iter().any(|a| a.id == id) {
            self.state.attempts.push(RemoteAttempt {
                id,
                kind: AttemptKind::Connect {
                    peer_id: peer_id.into(),
                },
            });
        }
        let events = self.events.clone();
        let name = peer_id.to_owned();
        self.spawn_job(async move {
            let paired = async {
                success(
                    send::<Discovery>(
                        network.as_ref(),
                        &peer,
                        Endpoint::Discover,
                        id,
                        cancel.child_token(),
                    )
                    .await?,
                )?;
                success(
                    send::<wire::PairResponse>(
                        network.as_ref(),
                        &peer,
                        Endpoint::Pair,
                        id,
                        cancel.child_token(),
                    )
                    .await?,
                )?;
                Ok(())
            }
            .await;
            let ready = paired.is_ok();
            let _ = events
                .send(Event::Paired {
                    epoch,
                    id,
                    peer: name.clone(),
                    result: paired,
                })
                .await;
            if ready {
                loop {
                    if cancel.is_cancelled() {
                        break;
                    }
                    let polled = send::<wire::PollResponse>(
                        network.as_ref(),
                        &peer,
                        Endpoint::Events,
                        id,
                        cancel.child_token(),
                    )
                    .await
                    .and_then(success);
                    match polled {
                        Ok(events) => {
                            // This settings/health view never attaches to output and
                            // must never acknowledge data that no client rendered.
                            if events.events.iter().any(|event| {
                                matches!(
                                    event,
                                    wire::HostEvent::Terminal {
                                        event: wire::TerminalEvent::Data { .. }
                                    }
                                )
                            }) {
                                return Event::PeerFailed {
                                    epoch,
                                    id,
                                    peer: name,
                                    error: error(
                                        ErrorCode::Unsupported,
                                        "Unowned terminal output on a control-only connection.",
                                    ),
                                };
                            }
                        }
                        Err(error) => {
                            return Event::PeerFailed {
                                epoch,
                                id,
                                peer: name,
                                error,
                            }
                        }
                    }
                }
            }
            Event::Finished
        });
        Ok(())
    }
    pub(super) fn disconnect(&mut self, peer_id: &str) {
        if let Some(connection) = self.connections.remove(peer_id) {
            connection.cancel.cancel();
            self.state.attempts.retain(|a| a.id != connection.id);
            if let (Some(network), Ok(peer)) = (
                self.network.clone(),
                wire::AuthenticatedPeerId::new(peer_id.to_owned()),
            ) {
                let id = connection.id;
                // Tombstone the exact old attempt even if its late pair reply was
                // lost. A replacement always receives another cryptographic UUID.
                self.spawn_job(async move {
                    let _ = send::<wire::Disconnected>(
                        network.as_ref(),
                        &peer,
                        Endpoint::Disconnect,
                        id,
                        CancellationToken::new(),
                    )
                    .await;
                    Event::Finished
                });
            }
        }
        if let Some(peer) = self.state.peers.iter_mut().find(|peer| peer.id == peer_id) {
            peer.connection = RemoteConnection::Disconnected;
            peer.attempt_id = None;
            peer.last_error = None;
        }
    }
}
