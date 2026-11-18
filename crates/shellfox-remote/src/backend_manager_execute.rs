use super::*;
impl Backend {
    pub(super) async fn manager_execute(
        &self,
        manager: &ManagerBackend,
        connection: ManagerConnection,
        command: Option<wire::Command>,
    ) -> Result<Value, wire::Error> {
        let polling = command.is_none();
        let _slot = Slot::enter(&manager.state, &connection, polling, (self.clock)())?;
        connection
            .actor()
            .validate_async()
            .await
            .map_err(application)?;
        if let Some(command) = command {
            crate::manager_policy::authorize(connection.actor(), &command).map_err(application)?;
            let request = ManagerOwnerRequest::new(
                connection.clone(),
                ManagerOperation::Execute(command.clone()),
            );
            let failure = request.failure.clone();
            let value = self
                .host
                .request(OwnerCommand::ManagerV2(Box::new(request)))
                .await
                .map_err(|error| {
                    failure.take().unwrap_or_else(|| {
                        if let wire::Command::WriteTerminal(input) = &command {
                            if !matches!(
                                error.code,
                                ErrorCode::AuthFailed | ErrorCode::AccessDenied
                            ) {
                                return input_unknown(
                                    input.input_id,
                                    "Input owner/write outcome is uncertain; do not retry input.",
                                );
                            }
                        }
                        application(error)
                    })
                })?;
            if let wire::Command::WriteTerminal(input) = &command {
                // A validated historical receipt is not a grant. Never discard actual
                // completed/ignored proof merely because revocation won after commit.
                crate::manager_policy::validate_result(&command, &value).map_err(|_| {
                    input_unknown(
                        input.input_id,
                        "Input receipt is malformed or mismatched; do not retry input.",
                    )
                })?;
                return Ok(value);
            }
            connection
                .actor()
                .validate_async()
                .await
                .map_err(application)?;
            connection
                .check(
                    &AuthenticatedPeerId::new(connection.actor().peer_id.clone())
                        .map_err(|_| application(auth()))?,
                    &connection.binding(),
                )
                .map_err(application)?;
            crate::manager_policy::validate_result(&command, &value).map_err(application)?;
            return Ok(value);
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            connection
                .actor()
                .validate_async()
                .await
                .map_err(application)?;
            connection
                .check(
                    &AuthenticatedPeerId::new(connection.actor().peer_id.clone())
                        .map_err(|_| application(auth()))?,
                    &connection.binding(),
                )
                .map_err(application)?;
            let value = self
                .host
                .request(OwnerCommand::ManagerV2(Box::new(ManagerOwnerRequest {
                    connection: connection.clone(),
                    operation: ManagerOperation::Poll,
                    failure: Default::default(),
                })))
                .await
                .map_err(application)?;
            let poll: wire::PollResponse =
                serde_json::from_value(value.clone()).map_err(|_| application(invalid()))?;
            poll.validate().map_err(|_| application(invalid()))?;
            connection
                .actor()
                .validate_async()
                .await
                .map_err(application)?;
            connection
                .check(
                    &AuthenticatedPeerId::new(connection.actor().peer_id.clone())
                        .map_err(|_| application(auth()))?,
                    &connection.binding(),
                )
                .map_err(application)?;
            if !poll.events.is_empty() || tokio::time::Instant::now() >= deadline {
                return Ok(value);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    /// Existing listener sweep uses this too when V2 is explicitly prepared.
    pub(crate) async fn sweep_manager_v2(&self) {
        let Some(manager) = &self.manager_v2 else {
            return;
        };
        let now = (self.clock)();
        let connections = match manager.state.lock() {
            Ok(state) => state
                .views
                .values()
                .map(|v| (v.connection.clone(), v.touched))
                .collect::<Vec<_>>(),
            Err(_) => return,
        };
        for (connection, touched) in connections {
            let current = AuthenticatedPeerId::new(connection.actor().peer_id.clone())
                .is_ok_and(|peer| connection.check(&peer, &connection.binding()).is_ok());
            if current
                && now.saturating_sub(touched) <= 60_000
                && connection.actor().validate_async().await.is_ok()
            {
                continue;
            }
            let retired = connection.clone();
            let _ = crate::approval_async::blocking(move || {
                retired.invalidate();
                Ok(())
            })
            .await;
            if self
                .host
                .request(OwnerCommand::ManagerV2(Box::new(ManagerOwnerRequest {
                    connection: connection.clone(),
                    operation: ManagerOperation::Disconnect,
                    failure: Default::default(),
                })))
                .await
                .is_ok()
            {
                if let Ok(mut state) = manager.state.lock() {
                    state.views.remove(&connection.binding().connection_id);
                }
            }
        }
        if let Ok(mut state) = manager.state.lock() {
            state
                .withdrawn
                .retain(|_, at| now.saturating_sub(*at) < 3_600_000);
            let live = state
                .views
                .keys()
                .copied()
                .collect::<std::collections::HashSet<_>>();
            state.attempts.retain(|_, attempt| {
                attempt.pairing
                    || attempt
                        .connection
                        .as_ref()
                        .is_some_and(|c| live.contains(&c.binding().connection_id))
                    || now.saturating_sub(attempt.touched) < 3_600_000
            });
        }
    }
}
