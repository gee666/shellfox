use super::*;
struct PairSlot {
    state: Arc<Mutex<Registry>>,
    key: String,
    connection: Option<ManagerConnection>,
    committed: bool,
}
impl Drop for PairSlot {
    fn drop(&mut self) {
        if !self.committed {
            if let Some(connection) = &self.connection {
                connection.invalidate();
            }
        }
        if let Ok(mut state) = self.state.lock() {
            if let Some(attempt) = state.attempts.get_mut(&self.key) {
                attempt.pairing = false;
            }
        }
    }
}
impl Backend {
    pub(super) async fn manager_pair(
        &self,
        manager: &ManagerBackend,
        peer: PeerIdentity,
        offer: wire::PairOffer,
        now: u64,
    ) -> Result<Value, wire::Error> {
        if offer.owner_id != manager.owner_id {
            return Err(application(auth()));
        }
        let key = attempt_key(&peer.id, offer.client_id, offer.attempt_id);
        let approval_client = {
            let mut state = manager.state.lock().map_err(|_| application(invalid()))?;
            if state.withdrawn.contains_key(&key) {
                return Err(application(auth()));
            }
            if !state.attempts.contains_key(&key) {
                if state.attempts.len() >= 128 {
                    return Err(application(invalid()));
                }
                state.attempts.insert(
                    key.clone(),
                    Attempt {
                        offer: offer.clone(),
                        approval_client: ClientId::new_v4(),
                        touched: now,
                        pairing: false,
                        connection: None,
                    },
                );
            }
            let attempt = state
                .attempts
                .get_mut(&key)
                .ok_or_else(|| application(auth()))?;
            if attempt.offer != offer || attempt.pairing {
                return Err(application(invalid()));
            }
            attempt.touched = now;
            attempt.pairing = true;
            attempt.approval_client
        };
        let mut slot = PairSlot {
            state: manager.state.clone(),
            key: key.clone(),
            connection: None,
            committed: false,
        };
        // Server-private attempt IDs isolate withdrawal from the unchanged V1
        // client namespace. Approval records/device permissions remain shared.
        let mut actor = self
            .approvals
            .authorize_async(peer.clone(), approval_client, now)
            .await
            .map_err(application)?;
        crate::manager_policy::validate_device(&actor).map_err(application)?;
        let existing = manager
            .state
            .lock()
            .map_err(|_| application(invalid()))?
            .attempts
            .get(&key)
            .and_then(|a| a.connection.clone());
        if let Some(existing) = existing {
            existing
                .actor()
                .validate_async()
                .await
                .map_err(application)?;
            let actual = AuthenticatedPeerId::new(peer.id).map_err(|_| application(auth()))?;
            existing
                .check(&actual, &existing.binding())
                .map_err(application)?;
            slot.committed = true;
            return checked(existing.selection().clone());
        }
        // Sender identity is SERVER state established by trusted composition,
        // not copied from the incoming offer or recovery metadata.
        let native0_sender = match offer.recovery_format {
            wire::RecoveryFormat::FormattedActiveBuffer => None,
            wire::RecoveryFormat::Native0RawV1 => {
                Some(manager.native0_sender.clone().ok_or_else(|| wire::Error {
                    code: ErrorCode::Unsupported,
                    message: "Native0 recovery is not prepared by this host.".into(),
                    retryable: false,
                    detail: wire::ErrorDetail::ProfileIncompatible,
                })?)
            }
        };
        // Internal owner/transfer incarnation only, never a terminal controller.
        {
            let mut limits = self.limits.lock().map_err(|_| application(invalid()))?;
            limits.epoch = limits
                .epoch
                .checked_add(1)
                .ok_or_else(|| application(invalid()))?;
            actor.view_epoch = limits.epoch;
        }
        let selection = wire::PairSelection {
            client_id: offer.client_id,
            attempt_id: offer.attempt_id,
            owner_id: offer.owner_id,
            connection_id: (manager.mint_connection)(),
            profile: offer.profile,
            catalog_mode: offer.catalog_mode,
            delivery_format: offer.delivery_format,
            recovery_format: offer.recovery_format,
            input_format: offer.input_format,
            device_policy: wire::DevicePolicy::ControlFullAccount,
            device_permission: wire::DevicePermission::Write,
            native0_sender,
            native0_receiver: offer.native0_receiver.clone(),
        };
        let connection = ManagerConnection::new(
            actor,
            AuthenticatedPeerId::new(peer.id.clone()).map_err(|_| application(auth()))?,
            selection,
        );
        slot.connection = Some(connection.clone());
        let replaced = {
            let mut state = manager.state.lock().map_err(|_| application(invalid()))?;
            if state.withdrawn.contains_key(&key) {
                return Err(application(auth()));
            }
            if state
                .views
                .contains_key(&connection.binding().connection_id)
            {
                return Err(application(invalid()));
            }
            let replaced = state
                .views
                .values()
                .filter(|view| {
                    view.connection.actor().peer_id == peer.id
                        && view.connection.binding().client_id == offer.client_id
                })
                .map(|view| view.connection.clone())
                .collect::<Vec<_>>();
            if state.views.len().saturating_sub(replaced.len()) >= 32 {
                return Err(application(invalid()));
            }
            state
                .attempts
                .get_mut(&key)
                .ok_or_else(|| application(auth()))?
                .connection = Some(connection.clone());
            state.views.insert(
                connection.binding().connection_id,
                ManagerView {
                    connection: connection.clone(),
                    polling: false,
                    operations: 0,
                    touched: now,
                },
            );
            replaced
        };
        for previous in replaced {
            let retired = previous.clone();
            crate::approval_async::blocking(move || {
                retired.invalidate();
                Ok(())
            })
            .await
            .map_err(application)?;
            self.host
                .request(OwnerCommand::ManagerV2(Box::new(ManagerOwnerRequest {
                    connection: previous.clone(),
                    operation: ManagerOperation::Disconnect,
                    failure: Default::default(),
                })))
                .await
                .map_err(application)?;
            manager
                .state
                .lock()
                .map_err(|_| application(invalid()))?
                .views
                .remove(&previous.binding().connection_id);
        }
        let value = self
            .host
            .request(OwnerCommand::ManagerV2(Box::new(ManagerOwnerRequest {
                connection: connection.clone(),
                operation: ManagerOperation::Pair,
                failure: Default::default(),
            })))
            .await
            .map_err(application)?;
        let accepted: wire::PairSelection =
            serde_json::from_value(value).map_err(|_| application(invalid()))?;
        accepted
            .validate_offer(&offer)
            .map_err(|_| application(auth()))?;
        if accepted.binding() != connection.binding() {
            return Err(application(auth()));
        }
        connection
            .actor()
            .validate_async()
            .await
            .map_err(application)?;
        connection
            .check(
                &AuthenticatedPeerId::new(peer.id.clone()).map_err(|_| application(auth()))?,
                &connection.binding(),
            )
            .map_err(application)?;
        let mut state = manager.state.lock().map_err(|_| application(invalid()))?;
        if state.withdrawn.contains_key(&key) {
            return Err(application(auth()));
        }
        let attempt = state
            .attempts
            .get_mut(&key)
            .ok_or_else(|| application(auth()))?;
        attempt.connection = Some(connection.clone());
        slot.committed = true;
        checked(connection.selection().clone())
    }
    pub(super) async fn manager_disconnect(
        &self,
        manager: &ManagerBackend,
        peer: PeerIdentity,
        request: wire::Disconnect,
        now: u64,
    ) -> Result<Value, wire::Error> {
        let (key, connection, approval_client) = {
            let mut state = manager.state.lock().map_err(|_| application(invalid()))?;
            let key = match request {
                wire::Disconnect::Attempt {
                    client_id,
                    attempt_id,
                    ..
                } => attempt_key(&peer.id, client_id, attempt_id),
                wire::Disconnect::Connection { binding, .. } => {
                    let connection = state
                        .views
                        .get(&binding.connection_id)
                        .map(|v| v.connection.clone())
                        .ok_or_else(|| application(auth()))?;
                    connection
                        .check(
                            &AuthenticatedPeerId::new(peer.id.clone())
                                .map_err(|_| application(auth()))?,
                            &binding,
                        )
                        .map_err(application)?;
                    attempt_key(
                        &peer.id,
                        binding.client_id,
                        connection.selection().attempt_id,
                    )
                }
            };
            if state.withdrawn.len() >= 256 && !state.withdrawn.contains_key(&key) {
                return Err(application(invalid()));
            }
            state.withdrawn.insert(key.clone(), now);
            let attempt = state.attempts.get(&key);
            (
                key,
                attempt.and_then(|a| a.connection.clone()),
                attempt.map(|a| a.approval_client),
            )
        };
        if let Some(connection) = connection {
            let retired = connection.clone();
            crate::approval_async::blocking(move || {
                retired.invalidate();
                Ok(())
            })
            .await
            .map_err(application)?;
            self.host
                .request(OwnerCommand::ManagerV2(Box::new(ManagerOwnerRequest {
                    connection: connection.clone(),
                    operation: ManagerOperation::Disconnect,
                    failure: Default::default(),
                })))
                .await
                .map_err(application)?;
            manager
                .state
                .lock()
                .map_err(|_| application(invalid()))?
                .views
                .remove(&connection.binding().connection_id);
        }
        if let Some(client) = approval_client {
            self.approvals
                .withdraw_async(peer, client, now)
                .await
                .map_err(application)?;
        }
        if let Some(attempt) = manager
            .state
            .lock()
            .map_err(|_| application(invalid()))?
            .attempts
            .get_mut(&key)
        {
            attempt.connection = None;
        }
        checked(wire::Disconnected { disconnected: true })
    }
}
