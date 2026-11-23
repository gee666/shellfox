use super::{remote::Operation, *};
impl SettingsPanel {
    pub(super) fn remote_action(&mut self, operation: Operation, cx: &mut Context<Self>) {
        if self.operations.frozen()
            || operation.client_access_target().is_some()
                && (self.remote.pending || self.remote.client_access_name(&operation).is_none())
        {
            return;
        }
        match operation {
            Operation::Advanced => {
                self.remote.advanced = !self.remote.advanced;
                cx.notify();
                return;
            }
            Operation::Revoke(id) => {
                self.remote.revoking = Some(id);
                cx.notify();
                return;
            }
            Operation::CancelRevoke => {
                self.remote.revoking = None;
                cx.notify();
                return;
            }
            Operation::CancelLogin => {
                if self.remote.pending && !self.remote.login_request {
                    return;
                }
                // Keep the request locked until its reply tells us which native
                // attempt to cancel. Discarding the reply can strand that attempt.
                self.remote.login.cancel();
                self.release_finished_login(cx);
                self.remote.error = None;
                cx.notify();
                return;
            }
            Operation::OpenLogin => {
                let Some(state) = &self.remote.state else {
                    return;
                };
                let was_busy = self.remote.login.busy();
                if let Some(serial) = self.remote.login.continue_browser(state) {
                    if !was_busy {
                        self.remote.login_started = Some(std::time::Instant::now());
                    }
                    self.login_timeout(serial, cx);
                    self.dispatch_login_browser(serial, cx);
                } else {
                    self.remote_action(Operation::Login, cx);
                }
                cx.notify();
                return;
            }
            _ => {}
        }
        if self.remote.pending {
            return;
        }
        let release = matches!(operation, Operation::Login)
            .then(|| self.remote.retry_release_id().map(str::to_owned))
            .flatten();
        if let Some(id) = release {
            self.remote.pending = true;
            self.remote.error = None;
            self.remote.epoch += 1;
            self.request_aux(
                api::Request::CancelAttempt(id),
                follow_up::After::RetryLogin(self.remote.epoch),
                cx,
            );
            return;
        }
        self.remote.connecting_peer = None;
        let mut login_request = false;
        let section = operation.section();
        let connecting = matches!(operation, Operation::Connect(_));
        let cancelling_pair = matches!(operation, Operation::CancelPair(_));
        let request = match operation {
            Operation::ToggleEnabled => {
                if self.remote.login.busy() {
                    return;
                }
                let Some(state) = &self.remote.state else {
                    return;
                };
                api::Request::ConfigureRemote {
                    enabled: !state.enabled,
                    hostname: state.hostname.clone(),
                }
            }
            Operation::SaveHostname => {
                let Some(state) = &self.remote.state else {
                    return;
                };
                let hostname = self.remote.hostname.read(cx).text.trim().to_owned();
                if hostname.is_empty() || hostname == state.hostname {
                    return;
                }
                api::Request::ConfigureRemote {
                    enabled: state.enabled,
                    hostname,
                }
            }
            Operation::Login | Operation::UseKey => {
                let Some(state) = &self.remote.state else {
                    return;
                };
                if login::blocked(state) || self.remote.login.busy() {
                    return;
                }
                let auth_key = if matches!(operation, Operation::UseKey) {
                    let key = self.remote.auth.read(cx).text.trim().to_owned();
                    if key.is_empty() {
                        return;
                    }
                    self.remote
                        .auth
                        .update(cx, |field, cx| field.set_text(String::new(), cx));
                    Some(key)
                } else {
                    None
                };
                let serial = self.remote.login.start(
                    state,
                    if auth_key.is_some() {
                        login::Kind::Key
                    } else {
                        login::Kind::Browser
                    },
                );
                self.remote.login_started = Some(std::time::Instant::now());
                self.login_timeout(serial, cx);
                self.start_remote_poll(cx);
                login_request = true;
                api::Request::Login { auth_key }
            }
            Operation::Connect(id) => {
                if !self.remote.state.as_ref().is_some_and(|state| {
                    state.peers.iter().any(|peer| {
                        peer.id == id
                            && peer.online
                            && matches!(
                                peer.connection,
                                api::Connection::Disconnected | api::Connection::Rejected
                            )
                    })
                }) {
                    return;
                }
                self.remote.connecting_peer = Some(id.clone());
                api::Request::Connect(id)
            }
            Operation::CancelAttempt(id) => {
                let Some(attempt) = self
                    .remote
                    .state
                    .as_ref()
                    .and_then(|state| state.attempts.iter().find(|attempt| attempt.id == id))
                else {
                    return;
                };
                if !matches!(attempt.kind, api::RemoteAttemptKind::Login) {
                    return;
                }
                self.remote.login.cancel();
                api::Request::CancelAttempt(id)
            }
            Operation::CancelPair(id) => api::Request::CancelAttempt(id),
            Operation::Disconnect(id) => {
                if !self.remote.connected(&id) {
                    return;
                }
                api::Request::Disconnect(id)
            }
            Operation::Approve(id) => api::Request::Approve(id),
            Operation::Reject(id) => api::Request::Reject(id),
            Operation::Forget(id) => api::Request::Forget(id),
            Operation::ConfirmRevoke(id) => {
                if self.remote.revoking.as_deref() != Some(id.as_str()) {
                    return;
                }
                self.remote.revoking = None;
                api::Request::Revoke(id)
            }
            Operation::Advanced
            | Operation::Revoke(_)
            | Operation::CancelRevoke
            | Operation::CancelLogin
            | Operation::OpenLogin => return,
        };
        self.remote.pending = true;
        self.remote.login_request = login_request;
        self.remote.error = None;
        self.remote.epoch += 1;
        let after = if login_request {
            follow_up::After::Login(self.remote.epoch)
        } else if connecting {
            follow_up::After::RemoteConnect {
                epoch: self.remote.epoch,
                generation: self.remote.pairing_scope.generation,
                started: std::time::Instant::now(),
            }
        } else if cancelling_pair {
            follow_up::After::RemoteCancel(self.remote.epoch)
        } else {
            follow_up::After::Remote(self.remote.epoch, section)
        };
        self.request_aux(request, after, cx);
    }
}
