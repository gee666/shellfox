use super::*;
impl Backend {
    pub(super) async fn observer_execute(
        &self,
        observer: &ObserverBackend,
        connection: ObserverConnection,
        command: Option<wire::Command>,
    ) -> Result<Value, wire::Error> {
        let polling = command.is_none();
        let _slot = Slot::enter(&observer.state, &connection, polling, (self.clock)())?;
        connection
            .actor()
            .validate_async()
            .await
            .map_err(application)?;
        if let Some(command) = command {
            command.authorize_profile()?;
            crate::observer_policy::authorize(connection.actor().permission, &command)
                .map_err(application)?;
            let value = self
                .host
                .request(OwnerCommand::ObserverV2(Box::new(ObserverOwnerRequest {
                    connection: connection.clone(),
                    operation: ObserverOperation::Execute(command.clone()),
                })))
                .await
                .map_err(application)?;
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
            crate::observer_policy::validate_result(&command, &value).map_err(application)?;
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
                .request(OwnerCommand::ObserverV2(Box::new(ObserverOwnerRequest {
                    connection: connection.clone(),
                    operation: ObserverOperation::Poll,
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
    pub(crate) async fn sweep_observer_v2(&self) {
        let Some(observer) = &self.observer_v2 else {
            return;
        };
        let now = (self.clock)();
        let connections = match observer.state.lock() {
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
                .request(OwnerCommand::ObserverV2(Box::new(ObserverOwnerRequest {
                    connection: connection.clone(),
                    operation: ObserverOperation::Disconnect,
                })))
                .await
                .is_ok()
            {
                if let Ok(mut state) = observer.state.lock() {
                    state.views.remove(&connection.binding().connection_id);
                }
            }
        }
        if let Ok(mut state) = observer.state.lock() {
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
