use super::*;
impl SettingsPanel {
    pub(super) fn start_remote_poll(&mut self, cx: &mut Context<Self>) {
        if self.remote.polling || !self.visible {
            return;
        }
        self.remote.polling = true;
        cx.spawn(async move |panel, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let keep = panel.update(cx, |panel, cx| {
                    if !panel.visible {
                        panel.remote.polling = false;
                        return false;
                    }
                    if panel.operations.frozen() {
                        return true;
                    }
                    panel.refresh(cx);
                    if panel.services.is_some() && !panel.remote.pending && !panel.remote.reading {
                        panel.remote.reading = true;
                        panel.request_aux(
                            api::Request::RemoteState,
                            aux::After::RemoteRead(panel.remote.epoch),
                            cx,
                        );
                    }
                    true
                });
                if !matches!(keep, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }
    pub(super) fn login_timeout(&mut self, serial: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |panel, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(120))
                .await;
            let _ = panel.update(cx, |panel, cx| {
                panel.remote.login.timeout(serial);
                panel.release_finished_login(cx);
                cx.notify();
            });
        })
        .detach();
    }
    // Release only an attempt returned by this panel's login request. A
    // browser-open failure still leaves that attempt available to continue.
    pub(super) fn release_finished_login(&mut self, cx: &mut Context<Self>) {
        if self.operations.frozen() || self.remote.pending || !self.remote.login.needs_release() {
            return;
        }
        if let Some(attempt) = self.remote.login_attempt.take() {
            self.remote.pending = true;
            self.remote.epoch += 1;
            self.request_aux(
                api::Request::CancelAttempt(attempt),
                aux::After::Remote(self.remote.epoch, remote::ErrorSection::Access),
                cx,
            );
        }
    }
    pub fn opened(&mut self, cx: &mut Context<Self>) {
        self.visible = true;
        self.focus_on_open = true;
        if !self.remote.pending {
            self.remote.login.dismissed();
        }
        self.refresh(cx);
        if self.services.is_some() {
            self.load_auxiliary(cx);
        }
        self.start_remote_poll(cx);
        cx.notify();
    }
    pub(super) fn request_close(&mut self, cx: &mut Context<Self>) {
        if self.operations.frozen() {
            return;
        }
        self.visible = false;
        self.color_picker = None;
        self.save_ready = true;
        self.begin_save(cx);
        // Closing Settings dismisses drafts and secrets for these forms, but
        // must not unlock a remote operation the host is still completing.
        if self.remote.login.phase != login::Phase::Cancelled {
            self.remote.login.dismissed();
        }
        self.remote
            .auth
            .update(cx, |field, cx| field.set_text(String::new(), cx));
        if let Some(state) = &self.remote.state {
            self.remote
                .hostname
                .update(cx, |field, cx| field.set_text(state.hostname.clone(), cx));
        }
        self.remote.advanced = false;
        self.remote.revoking = None;
        self.remote.error = None;
        self.profile_menu = false;
        if !self.ssh.pending {
            self.ssh.clear_editor();
            self.ssh.focus_control = None;
            self.ssh.error = None;
        }
        self.environment = None;
        self.environment_subscription = None;
        cx.emit(SettingsEvent::CloseRequested);
        cx.notify();
    }
}
