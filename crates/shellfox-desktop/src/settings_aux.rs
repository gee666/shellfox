use super::*;
#[derive(Clone, Copy)]
pub(super) enum After {
    Remote(u64),
    RemoteRead(u64),
    Login(u64),
    SshRead,
    Ssh,
    Import,
    Key,
}
impl SettingsPanel {
    pub(super) fn load_auxiliary(&mut self, cx: &mut Context<Self>) {
        if !self.remote.pending && !self.remote.reading {
            self.remote.reading = true;
            self.request_aux(
                api::Request::RemoteState,
                After::RemoteRead(self.remote.epoch),
                cx,
            );
        }
        if !self.ssh.pending && self.ssh.editor.is_none() && self.ssh.removing.is_none() {
            self.request_aux(api::Request::SshList, After::SshRead, cx);
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
            );
            if !self.ssh.pending
                && let Some(editor) = &self.ssh.editor
            {
                editor.configure_fields(self.operations.frozen(), cx);
            }
            cx.notify();
            return;
        };
        let lease = mutating.then(|| self.operations.begin());
        cx.spawn(async move |panel, cx| {
            let result = services.request(request).await;
            let _ = panel.update(cx, |panel, cx| {
                drop(lease);
                if matches!(after, After::RemoteRead(_)) {
                    panel.remote.reading = false;
                }
                if let After::Remote(epoch) | After::RemoteRead(epoch) | After::Login(epoch) = after
                    && epoch != panel.remote.epoch
                {
                    return;
                }
                match after {
                    After::Remote(_) | After::Login(_) => {
                        panel.remote.pending = false;
                        panel.remote.inflight_peer = None;
                        panel.remote.login_request = false;
                    }
                    After::Ssh | After::Import | After::Key => panel.ssh.pending = false,
                    _ => {}
                }
                match result {
                    Ok(response) => panel.accept_aux(after, response, cx),
                    Err(error) => panel.aux_error(after, error.to_string()),
                }
                if !panel.ssh.pending
                    && let Some(editor) = &panel.ssh.editor
                {
                    editor.configure_fields(panel.operations.frozen(), cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn aux_error(&mut self, after: After, message: String) {
        match after {
            After::Login(_) => {
                self.remote.pending = false;
                self.remote.login.fail();
                self.remote.error = Some(message);
            }
            After::Remote(_) => {
                self.remote.pending = false;
                self.remote.error = Some(message);
            }
            After::RemoteRead(_) => {
                self.remote.reading = false;
                self.remote.error = Some(message);
            }
            After::SshRead => {
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
                After::Remote(_) | After::RemoteRead(_) | After::Login(_),
                api::Response::Remote(state),
            ) => {
                let current = self.remote.hostname.read(cx).text.clone();
                if self
                    .remote
                    .state
                    .as_ref()
                    .is_none_or(|old| old.hostname == current)
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
                let open = !self.operations.frozen()
                    && self
                        .remote
                        .login
                        .accept(&state, matches!(after, After::Login(_)));
                self.remote.state = Some(state);
                self.remote.error = None;
                if open {
                    let serial = self.remote.login.serial();
                    self.dispatch_login_browser(serial, cx);
                }
            }
            (After::SshRead, api::Response::SshProfiles(profiles)) => {
                self.ssh.profiles = Some(profiles);
                self.ssh.error = None;
            }
            (After::Ssh, api::Response::SshProfiles(profiles)) => {
                self.ssh.profiles = Some(profiles);
                self.ssh.editor = None;
                self.ssh.removing = None;
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
            ),
        }
    }
    pub(super) fn choose_color(&mut self, kind: api::ColorKind, cx: &mut Context<Self>) {
        if self.operations.frozen() {
            return;
        }
        let target = match kind {
            api::ColorKind::Accent => "Accent",
            api::ColorKind::Background => "Background",
        };
        self.auxiliary_error = Some(format!(
            "{target} color picker awaits the Chromium interaction reference. No replacement picker was opened."
        ));
        cx.notify();
    }
}
