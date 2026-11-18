use super::*;
impl SshManager {
    /// Called on the SAME service owner tick. Input/view leases are not native authority.
    pub fn poll(
        &mut self,
        validator: &dyn NativeControllerValidator,
        starter: &mut dyn OwnedRuntimeStarter,
    ) {
        for c in self.connections.values_mut() {
            if self.lost.is_cancelled()
                || (!c.authenticated
                    && (!c.view_attached || validator.validate_native(c.controller).is_err()))
            {
                c.stop();
            } else if c.authenticated
                && !c.cancel.is_cancelled()
                && c.view_attached
                && validator.validate_native(c.controller).is_err()
            {
                c.detach();
            }
        }
        for _ in 0..128 {
            let Ok(event) = self.incoming.try_recv() else {
                break;
            };
            match event {
                WorkEvent::Host {
                    connection,
                    value,
                    reply,
                } => {
                    if let Some(c) = self.connections.get_mut(&connection).filter(|c| {
                        !c.cancel.is_cancelled()
                            && !c.auth_cancel.is_cancelled()
                            && c.view_attached
                            && !c.authenticated
                    }) {
                        c.pending = Some(Pending::Host(value.challenge, reply));
                        c.status.state = State::AwaitingHost(value);
                    }
                }
                WorkEvent::Prompt {
                    connection,
                    value,
                    reply,
                } => {
                    if let Some(c) = self.connections.get_mut(&connection).filter(|c| {
                        !c.cancel.is_cancelled() && !c.auth_cancel.is_cancelled() && c.view_attached
                    }) {
                        c.pending = Some(Pending::Prompt(value.prompt, reply));
                        c.status.state = State::AwaitingPrompt(value);
                    }
                }
                WorkEvent::Prepared { connection, result } => {
                    let Some(c) = self.connections.get_mut(&connection).filter(|c| {
                        !c.cancel.is_cancelled()
                            && !c.auth_cancel.is_cancelled()
                            && c.view_attached
                            && c.runtime.is_none()
                            && c.reserved
                    }) else {
                        continue;
                    };
                    c.pending = None;
                    let mut prepared = match result {
                        Ok(v) => v,
                        Err(error) => {
                            let message = error
                                .downcast_ref::<shellfox_ssh::Error>()
                                .map(ToString::to_string)
                                .unwrap_or_else(|| {
                                    "SSH connection preparation failed or was cancelled.".into()
                                });
                            c.stop();
                            c.status.state = State::Failed { message };
                            continue;
                        }
                    };
                    if self.lost.is_cancelled()
                        || validator.validate_native(c.controller).is_err()
                        || validator
                            .validate_target(c.controller, c.target.tab())
                            .is_err()
                    {
                        c.stop();
                        continue;
                    }
                    let Some(mut launch) = prepared.launch.take() else {
                        c.stop();
                        continue;
                    };
                    let identity_secret = prepared
                        .identity
                        .as_ref()
                        .map(|i| (i.path().to_owned(), i.passphrase().clone()));
                    let work = Work {
                        connection,
                        events: self.events.clone(),
                        cancel: c.auth_cancel.clone(),
                        lost: self.lost.clone(),
                    };
                    let broker = match broker::Broker::start(
                        self.directory.path(),
                        &self.askpass,
                        work,
                        launch.authentication.saved_password.take(),
                        identity_secret,
                    ) {
                        Ok(b) => b,
                        Err(_) => {
                            c.stop();
                            c.status.state = State::Failed {
                                message: "Private SSH authentication broker could not start."
                                    .into(),
                            };
                            continue;
                        }
                    };
                    if self.lost.is_cancelled()
                        || validator.validate_native(c.controller).is_err()
                        || validator
                            .validate_target(c.controller, c.target.tab())
                            .is_err()
                    {
                        drop(broker);
                        c.stop();
                        continue;
                    }
                    c.status.state = State::Starting;
                    match starter.start(c.controller, c.target, &launch, &broker.environment) {
                        Ok(identity) if identity.tab_id == c.target.tab() => {
                            c.runtime = Some(identity);
                            c.broker = Some(broker);
                            c.identity = prepared.identity.take();
                            c.proof = Some(launch.expected_authentication);
                            c.status.state = State::Authenticating;
                        }
                        Ok(identity) => {
                            // Retain even a bad provider result until its actual generation is closed.
                            c.runtime = Some(identity);
                            c.broker = Some(broker);
                            c.identity = prepared.identity.take();
                            c.stop();
                        }
                        Err(_) => {
                            drop(broker);
                            c.stop();
                            c.status.state = State::Failed {
                                message: "Owned SSH runtime could not start.".into(),
                            };
                        }
                    }
                }
            }
        }
        for c in self.connections.values_mut() {
            if c.job.as_ref().is_some_and(JoinHandle::is_finished) {
                if let Some(job) = c.job.take() {
                    if job.join().is_err() {
                        c.stop();
                        c.status.state = State::Failed {
                            message: "SSH preparation worker stopped.".into(),
                        };
                    }
                }
            }
            if !c.cancel.is_cancelled() {
                continue;
            }
            c.pending = None;
            if let Some(identity) = c.runtime {
                if matches!(starter.close(identity), Ok(true)) {
                    c.runtime = None;
                } else {
                    continue;
                }
            }
            if c.job.is_some() {
                continue;
            }
            if let Some(broker) = &mut c.broker {
                if broker.stop().is_err() {
                    continue;
                }
            }
            c.broker = None;
            if let Some(identity) = &c.identity {
                if identity.cleanup().is_err() {
                    continue;
                }
            }
            c.identity = None;
            c.proof = None;
            if c.reserved {
                if starter.release_target(c.target).is_err() {
                    continue;
                }
                c.reserved = false;
            }
            if !matches!(c.status.state, State::Failed { .. }) {
                c.status.state = State::Closed;
            }
        }
    }
}
