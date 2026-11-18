use super::*;
impl Manager {
    pub(super) async fn request(&mut self, request: RemoteRequest) -> Result<(), AppError> {
        request.validate()?;
        match &request {
            RemoteRequest::Disconnect { peer_id }
            | RemoteRequest::Reconnect { peer_id }
            | RemoteRequest::Revoke { peer_id }
            | RemoteRequest::Reject { peer_id }
            | RemoteRequest::Forget { peer_id } => self.cancel_outgoing_peer(peer_id),
            _ => {}
        }
        if self.jobs.len() >= 64
            && !matches!(
                &request,
                RemoteRequest::State
                    | RemoteRequest::Cancel { .. }
                    | RemoteRequest::Disconnect { .. }
                    | RemoteRequest::Configure { enabled: false, .. }
                    | RemoteRequest::Revoke { .. }
                    | RemoteRequest::Reject { .. }
            )
        {
            return Err(error(
                ErrorCode::NativeUnavailable,
                "Remote operation queue is full. Action was not accepted.",
            ));
        }
        match request {
            RemoteRequest::State => {}
            RemoteRequest::Configure { enabled, hostname } => {
                self.configure(enabled, hostname).await?
            }
            RemoteRequest::Login { auth_key } => {
                if !self.state.enabled
                    || self.network.is_none()
                    || !self.owned_jobs.is_empty()
                    || self.state.cleanup_retained
                {
                    return Err(error(
                        ErrorCode::Unsupported,
                        "Enable remote networking first.",
                    ));
                }
                if self.health_busy
                    || self
                        .state
                        .attempts
                        .iter()
                        .any(|a| matches!(a.kind, AttemptKind::Login))
                {
                    return Err(error(
                        ErrorCode::Validation,
                        "A login or status request is already pending.",
                    ));
                }
                let id = Uuid::new_v4();
                self.state.attempts.push(RemoteAttempt {
                    id,
                    kind: AttemptKind::Login,
                });
                self.health(Some(id), Some(auth_key));
            }
            RemoteRequest::Cancel { attempt_id } => {
                let attempt = self
                    .state
                    .attempts
                    .iter()
                    .find(|a| a.id == attempt_id)
                    .cloned()
                    .ok_or_else(|| error(ErrorCode::NotFound, "Attempt is no longer current."))?;
                match attempt.kind {
                    AttemptKind::Connect { peer_id } => {
                        if !self
                            .connections
                            .get(&peer_id)
                            .is_some_and(|connection| connection.id == attempt_id)
                        {
                            return Err(error(
                                ErrorCode::NotFound,
                                "Connection attempt was replaced.",
                            ));
                        }
                        self.disconnect(&peer_id)
                    }
                    AttemptKind::Resume => {}
                    AttemptKind::Login => {
                        self.state.login_url = None;
                    }
                    AttemptKind::Enable => {
                        self.backend.approvals().set_enabled_async(false).await?;
                        self.cycle.cancel();
                        self.epoch = self
                            .epoch
                            .checked_add(1)
                            .ok_or_else(|| error(ErrorCode::Internal, "Remote epoch exhausted."))?;
                        self.state.enabled = false;
                        self.mark_stopping();
                        self.retry_retained();
                    }
                }
                self.state.attempts.retain(|a| a.id != attempt_id);
            }
            RemoteRequest::Continue { attempt_id } => {
                let attempt = self
                    .state
                    .attempts
                    .iter()
                    .find(|a| a.id == attempt_id)
                    .cloned()
                    .ok_or_else(|| error(ErrorCode::NotFound, "Attempt is no longer current."))?;
                match attempt.kind {
                    AttemptKind::Resume => {
                        let hostname = self.state.hostname.clone();
                        Box::pin(self.request(RemoteRequest::Configure {
                            enabled: true,
                            hostname,
                        }))
                        .await?;
                    }
                    AttemptKind::Login => {
                        if self.health_busy {
                            return Err(error(
                                ErrorCode::Validation,
                                "Status request already pending.",
                            ));
                        }
                        self.health(Some(attempt_id), None);
                    }
                    AttemptKind::Connect { peer_id } => self.connect(&peer_id, Some(attempt_id))?,
                    AttemptKind::Enable => {
                        return Err(error(ErrorCode::Validation, "Enable is still pending."))
                    }
                }
            }
            RemoteRequest::Connect { peer_id } => {
                self.remember(&peer_id).await?;
                self.connect(&peer_id, None)?;
            }
            RemoteRequest::Reconnect { peer_id } => {
                self.disconnect(&peer_id);
                self.remember(&peer_id).await?;
                self.connect(&peer_id, None)?;
            }
            RemoteRequest::Disconnect { peer_id } => self.disconnect(&peer_id),
            RemoteRequest::Forget { peer_id } => {
                self.disconnect(&peer_id);
                let mut preferences = self.preferences.clone();
                preferences.peers.retain(|peer| peer.id != peer_id);
                self.backend.save_preferences(preferences.clone()).await?;
                self.preferences = preferences;
                self.state.peers.retain(|p| p.id != peer_id);
            }
            RemoteRequest::Approve { peer_id } => {
                self.backend.approve_full_account(&peer_id).await?
            }
            RemoteRequest::Reject { peer_id } => {
                self.backend.approvals().reject_async(peer_id).await?
            }
            RemoteRequest::Revoke { peer_id } => self.backend.revoke(&peer_id).await?,
        }
        Ok(())
    }
    async fn remember(&mut self, peer_id: &str) -> Result<(), AppError> {
        let peer = self
            .state
            .peers
            .iter()
            .find(|peer| peer.id == peer_id)
            .ok_or_else(|| error(ErrorCode::NotFound, "Peer has not been discovered."))?;
        let mut preferences = self.preferences.clone();
        preferences.peers.retain(|peer| peer.id != peer_id);
        preferences
            .peers
            .push(super::super::preferences::RememberedPeer {
                id: peer.id.clone(),
                hostname: peer.hostname.clone(),
                addresses: peer.addresses.clone(),
            });
        self.backend.save_preferences(preferences.clone()).await?;
        self.preferences = preferences;
        Ok(())
    }
}
