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
                    if panel.services.is_some()
                        && (panel.category == Category::RemoteAccess || panel.remote.login.busy())
                        && !panel.remote.pending
                        && !panel.remote.reading
                    {
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
                cx.notify();
            });
        })
        .detach();
    }
    pub fn opened(&mut self, cx: &mut Context<Self>) {
        self.visible = true;
        self.focus_on_open = true;
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
        self.save_ready = true;
        self.begin_save(cx);
        self.remote.login.cancel();
        self.remote.epoch += 1;
        self.remote.pending = false;
        self.remote.login_request = false;
        self.remote.inflight_peer = None;
        self.remote.reading = false;
        self.profile_menu = false;
        if !self.ssh.pending {
            self.ssh.cancel_editor();
        }
        self.environment = None;
        self.environment_subscription = None;
        cx.emit(SettingsEvent::CloseRequested);
        cx.notify();
    }
}
