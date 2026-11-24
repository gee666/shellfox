use super::*;
impl Manager {
    pub(super) async fn configure(
        &mut self,
        enabled: bool,
        hostname: String,
    ) -> Result<(), AppError> {
        if enabled
            && (!self.owned_jobs.is_empty() || self.orphan.is_some() || self.state.cleanup_retained)
        {
            return Err(cleanup_retained());
        }
        self.backend.approvals().set_enabled_async(false).await?;
        let mut preferences = self.preferences.clone();
        preferences.enabled = enabled;
        preferences.hostname = hostname.clone();
        self.backend.save_preferences(preferences.clone()).await?;
        self.preferences = preferences;
        self.cycle.cancel();
        self.cycle = CancellationToken::new();
        self.connections.clear();
        self.discovery.clear();
        self.probe_busy = false;
        self.health_busy = false;
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or_else(|| error(ErrorCode::Internal, "Remote epoch exhausted."))?;
        self.state.enabled = enabled;
        self.state.hostname = hostname.clone();
        self.state.login_url = None;
        self.state.error = None;
        self.state.attempts.clear();
        for peer in &mut self.state.peers {
            peer.connection = RemoteConnection::Disconnected;
            peer.attempt_id = None;
            peer.online = false;
        }
        if enabled {
            crate::remote::v1_privilege::note_root_remote_access();
            let id = Uuid::new_v4();
            self.state.attempts.push(RemoteAttempt {
                id,
                kind: AttemptKind::Enable,
            });
            self.state.status = RemoteNetworkStatus::Starting;
            let config = self
                .backend
                .helper_config(self.directory.clone(), hostname)?;
            let old = self.network.clone();
            let factory = self.factory.clone();
            let epoch = self.epoch;
            // A helper lifecycle future must survive UI cancellation. Its result
            // is either adopted at this epoch or stopped with retained ownership.
            self.owned_jobs.spawn(async move {
                let previous_confirmed = if let Some(old) = old {
                    old.shutdown().await.is_ok()
                } else {
                    true
                };
                let result = if previous_confirmed {
                    factory.start(config).await
                } else {
                    Err(cleanup_retained())
                };
                Event::Started {
                    epoch,
                    id,
                    previous_confirmed,
                    result,
                }
            });
        } else {
            self.mark_stopping();
            self.retry_retained();
        }
        Ok(())
    }
    pub(super) fn mark_stopping(&mut self) {
        self.state.cleanup_retained =
            self.network.is_some() || self.orphan.is_some() || !self.owned_jobs.is_empty();
        self.state.status = if self.state.cleanup_retained {
            RemoteNetworkStatus::Error
        } else {
            RemoteNetworkStatus::Disabled
        };
        self.state.error = self.state.cleanup_retained.then(cleanup_retained);
    }
    pub(super) fn retry_retained(&mut self) {
        if !self.owned_jobs.is_empty() {
            return;
        }
        let selected = if let Some(network) = &self.orphan {
            Some((true, network.clone()))
        } else if !self.state.enabled || self.state.cleanup_retained {
            self.network.clone().map(|network| (false, network))
        } else {
            None
        };
        if let Some((orphan, network)) = selected {
            self.owned_jobs.spawn(async move {
                let result = tokio::time::timeout(Duration::from_secs(5), network.shutdown())
                    .await
                    .unwrap_or_else(|_| Err(cleanup_retained()));
                Event::Stopped {
                    orphan,
                    network,
                    result,
                }
            });
        }
    }
    pub(super) async fn lifecycle_event(&mut self, event: Event) {
        match event {
            Event::Started {
                epoch,
                id,
                previous_confirmed,
                result,
            } => {
                if previous_confirmed {
                    self.network.take();
                }
                let current = epoch == self.epoch
                    && self.state.enabled
                    && self.state.attempts.iter().any(|attempt| attempt.id == id);
                if current {
                    self.state.attempts.retain(|attempt| attempt.id != id);
                    match result {
                        Ok(network) => {
                            self.network = Some(network);
                            self.state.cleanup_retained = false;
                            if let Err(error) =
                                self.backend.approvals().set_enabled_async(true).await
                            {
                                self.state.error = Some(error);
                                self.state.status = RemoteNetworkStatus::Error;
                            } else {
                                self.health(None, None);
                            }
                        }
                        Err(error) => {
                            self.state.cleanup_retained = !previous_confirmed;
                            self.state.status = RemoteNetworkStatus::Error;
                            self.state.error = Some(error);
                        }
                    }
                } else {
                    if let Ok(network) = result {
                        self.orphan = Some(network);
                    }
                    self.mark_stopping();
                    self.retry_retained();
                }
            }
            Event::Stopped {
                orphan,
                network,
                result,
            } => {
                if result.is_ok() {
                    let slot = if orphan {
                        &mut self.orphan
                    } else {
                        &mut self.network
                    };
                    if slot
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, &network))
                    {
                        slot.take();
                    }
                }
                self.mark_stopping();
            }
            _ => {}
        }
    }
}
