use super::*;
#[derive(Clone, Copy)]
pub(super) enum After {
    Remote(u64, remote::ErrorSection),
    RemoteConnect {
        epoch: u64,
        generation: u64,
        started: std::time::Instant,
    },
    RemoteCancel(u64),
    RemoteRead(u64),
    Login(u64),
    RetryLogin(u64),
    SshRead(u64),
    Ssh,
    Import,
    Key,
}
impl SettingsPanel {
    pub(super) fn load_remote(&mut self, cx: &mut Context<Self>) {
        if !self.remote.pending && !self.remote.reading && fixture::value("REMOTE").is_none() {
            self.remote.reading = true;
            self.request_aux(
                api::Request::RemoteState,
                After::RemoteRead(self.remote.epoch),
                cx,
            );
        }
    }

    pub(super) fn load_auxiliary(&mut self, cx: &mut Context<Self>) {
        self.load_remote(cx);
        if !self.ssh.reading
            && !self.ssh.pending
            && self.ssh.editor.is_none()
            && self.ssh.removing.is_none()
        {
            self.ssh.reading = true;
            self.request_aux(api::Request::SshList, After::SshRead(self.ssh.epoch), cx);
        }
    }
    pub(super) fn request_aux(
        &mut self,
        request: api::Request,
        after: After,
        cx: &mut Context<Self>,
    ) {
        let mutating = operations::mutating(&request);
        if mutating && self.operations.frozen() {
            return;
        }
        let Some(services) = self.services.clone() else {
            self.aux_error(
                after,
                "This settings capability is unavailable on the connected host.".into(),
                cx,
            );
            if let Some(editor) = self.ssh.editor.as_ref().filter(|_| !self.ssh.pending) {
                editor.configure_fields(self.operations.frozen(), cx);
            }
            cx.notify();
            return;
        };
        if matches!(after, After::Ssh | After::Import) {
            self.ssh.epoch += 1;
        }
        let lease = mutating.then(|| self.operations.begin());
        cx.spawn(async move |panel, cx| {
            let result = services.request(request).await;
            let _ = panel.update(cx, |panel, cx| {
                drop(lease);
                if matches!(after, After::RemoteRead(_)) {
                    panel.remote.reading = false;
                }
                match after {
                    After::RemoteRead(epoch) if epoch != panel.remote.epoch => {
                        if panel.visible && !panel.operations.frozen() {
                            panel.load_remote(cx);
                        }
                        return;
                    }
                    After::Remote(epoch, _)
                    | After::RemoteConnect { epoch, .. }
                    | After::RemoteCancel(epoch)
                    | After::Login(epoch)
                    | After::RetryLogin(epoch)
                        if epoch != panel.remote.epoch =>
                    {
                        return;
                    }
                    _ => {}
                }
                if let After::SshRead(epoch) = after {
                    panel.ssh.reading = false;
                    if epoch != panel.ssh.epoch
                        || panel.ssh.editor.is_some()
                        || panel.ssh.removing.is_some()
                    {
                        return;
                    }
                }
                match after {
                    After::Remote(_, _)
                    | After::RemoteConnect { .. }
                    | After::RemoteCancel(_)
                    | After::Login(_)
                    | After::RetryLogin(_) => {
                        panel.remote.pending = false;
                        if !matches!(after, After::RemoteConnect { .. }) {
                            panel.remote.connecting_peer = None;
                        }
                        panel.remote.login_request = false;
                    }
                    After::Ssh | After::Import | After::Key => panel.ssh.pending = false,
                    _ => {}
                }
                match result {
                    Ok(response) => panel.accept_aux(after, response, cx),
                    Err(error) => panel.aux_error(after, error.to_string(), cx),
                }
                if let Some(editor) = panel.ssh.editor.as_ref().filter(|_| !panel.ssh.pending) {
                    editor.configure_fields(panel.operations.frozen(), cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn aux_error(&mut self, after: After, message: String, cx: &mut Context<Self>) {
        match after {
            After::Login(_) => {
                self.remote.pending = false;
                self.remote.login.fail();
            }
            After::Remote(_, section) => {
                self.remote.pending = false;
                if self.visible {
                    self.remote.fail(section, &message);
                }
            }
            After::RemoteConnect { .. } => {
                self.remote.pending = false;
                self.remote.connecting_peer = None;
                if self.visible {
                    self.remote.fail(remote::ErrorSection::Machines, &message);
                }
            }
            After::RetryLogin(_) => {
                self.remote.pending = false;
                if self.visible {
                    self.remote.fail(remote::ErrorSection::Access, &message);
                }
            }
            After::RemoteCancel(_) => {
                self.remote.pending = false;
                if self.visible {
                    self.remote.fail(remote::ErrorSection::Machines, &message);
                    if self.services.is_some() && !self.operations.frozen() {
                        self.load_remote(cx);
                    }
                }
            }
            After::RemoteRead(_) => {
                self.remote.reading = false;
                self.remote.read_error = Some(message);
            }
            After::SshRead(_) => {
                self.ssh.reading = false;
                self.ssh.error = Some(message);
            }
            After::Ssh | After::Import | After::Key => {
                self.ssh.pending = false;
                self.ssh.error = Some(message);
            }
        }
    }
    fn accept_aux(&mut self, after: After, response: api::Response, cx: &mut Context<Self>) {
        match (after, response) {
            (
                After::RemoteConnect {
                    epoch,
                    generation,
                    started,
                },
                api::Response::ConnectedRequest { state, result },
            ) => {
                if let Some(peer) = self.remote.connecting_peer.take() {
                    self.watch_pairing(peer, &state, result, started, generation, cx);
                }
                self.accept_aux(
                    After::Remote(epoch, remote::ErrorSection::Machines),
                    api::Response::Remote(state),
                    cx,
                );
            }
            (After::RemoteConnect { epoch, .. }, api::Response::Remote(state)) => {
                // Attached hosts have an unchanged wire reply and no creation receipt.
                self.remote.connecting_peer = None;
                self.accept_aux(
                    After::Remote(epoch, remote::ErrorSection::Machines),
                    api::Response::Remote(state),
                    cx,
                );
            }
            (
                After::Remote(_, _)
                | After::RemoteCancel(_)
                | After::RemoteRead(_)
                | After::Login(_)
                | After::RetryLogin(_),
                api::Response::Remote(state),
            ) => {
                if self
                    .remote
                    .state
                    .as_ref()
                    .is_none_or(|old| old.hostname != state.hostname)
                {
                    self.remote
                        .hostname
                        .update(cx, |field, cx| field.set_text(state.hostname.clone(), cx));
                }
                if self.remote.login_started.is_none()
                    && state.attempts.iter().any(|attempt| {
                        matches!(
                            attempt.kind,
                            api::RemoteAttemptKind::Enable | api::RemoteAttemptKind::Login
                        )
                    })
                {
                    self.remote.login_started = Some(std::time::Instant::now());
                }
                let open = self.visible
                    && !self.operations.frozen()
                    && self
                        .remote
                        .login
                        .accept(&state, matches!(after, After::Login(_)));
                if matches!(after, After::Login(_)) {
                    self.remote.login_attempt = state
                        .attempts
                        .iter()
                        .find(|attempt| matches!(attempt.kind, api::RemoteAttemptKind::Login))
                        .map(|attempt| attempt.id.clone());
                } else if self
                    .remote
                    .login_attempt
                    .as_ref()
                    .is_some_and(|id| !state.attempts.iter().any(|attempt| attempt.id == *id))
                {
                    self.remote.login_attempt = None;
                }
                let retry = matches!(after, After::RetryLogin(_))
                    && self.visible
                    && self.remote.login_attempt.is_none();
                self.remote.pairing_scope.retain_attempts(&state);
                self.remote.state = Some(state);
                self.remote.read_error = None;
                if retry {
                    self.remote_action(remote::Operation::Login, cx);
                    return;
                }
                self.release_finished_login(cx);
                if open {
                    let serial = self.remote.login.serial();
                    self.dispatch_login_browser(serial, cx);
                }
            }
            (After::SshRead(_), api::Response::SshProfiles(profiles)) => {
                self.ssh.profiles = Some(profiles);
                self.ssh.error = None;
            }
            (After::Ssh, api::Response::SshProfiles(profiles)) => {
                self.ssh.focus_control = self.visible.then(|| {
                    self.ssh
                        .editor
                        .as_ref()
                        .map_or_else(|| "add-ssh".into(), |editor| ssh::row_id(&editor.id))
                });
                self.ssh.profiles = Some(profiles);
                self.ssh.clear_editor();
                self.ssh.error = None;
            }
            (
                After::Import,
                api::Response::SshImported {
                    profiles,
                    found,
                    added,
                    updated,
                },
            ) => {
                self.ssh.profiles = Some(profiles);
                self.ssh.status = Some(if found == 0 {
                    "No PuTTY sessions found".into()
                } else if added == 0 && updated == 0 {
                    "Already up to date".into()
                } else {
                    format!("{added} added, {updated} updated")
                });
                self.ssh.status_epoch += 1;
                let epoch = self.ssh.status_epoch;
                cx.spawn(async move |panel, cx| {
                    cx.background_executor().timer(Duration::from_secs(3)).await;
                    let _ = panel.update(cx, |panel, cx| {
                        if panel.ssh.status_epoch == epoch {
                            panel.ssh.status = None;
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
            (After::Key, api::Response::KeyFile(Some(path))) => {
                if let Some(editor) = &self.ssh.editor {
                    editor.key.update(cx, |field, cx| field.set_text(path, cx));
                }
            }
            (After::Key, api::Response::KeyFile(None)) => {}
            _ => self.aux_error(
                after,
                "Unexpected settings capability response from host.".into(),
                cx,
            ),
        }
    }
}
