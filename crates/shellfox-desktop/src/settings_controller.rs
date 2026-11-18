use super::*;
impl SettingsPanel {
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.operations.frozen()
            || self.loading
            || self.environment_writing
            || self.environment_write.is_some()
            || self
                .draft
                .as_ref()
                .is_some_and(|draft| draft.inflight.is_some())
        {
            return;
        }
        self.loading = true;
        let client = self.client.clone();
        cx.spawn(async move |panel, cx| {
            let result = client
                .execute(NativeAction::Settings(
                    shellfox_host::SettingsRequest::Get {},
                ))
                .await;
            let _ = panel.update(cx, |panel, cx| {
                panel.loading = false;
                match result {
                    Ok(NativeResponse::Settings(settings)) => {
                        panel.accept_settings(settings, false, cx)
                    }
                    Ok(_) => panel.load_error = Some("Invalid settings response from host.".into()),
                    Err(error) => panel.load_error = Some(error.to_string()),
                }
                panel.begin_save(cx);
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn accept_settings(
        &mut self,
        settings: Settings,
        saved: bool,
        cx: &mut Context<Self>,
    ) {
        let result = if let Some(draft) = &mut self.draft {
            if saved {
                draft.saved(settings.clone())
            } else {
                draft.observed(settings.clone())
            }
        } else {
            state::Draft::new(settings.clone()).map(|draft| self.draft = Some(draft))
        };
        if let Err(error) = result {
            let message = error.to_string();
            if saved && let Some(draft) = &mut self.draft {
                draft.failed(message.clone());
            }
            self.load_error = Some(message);
            return;
        }
        self.load_error = None;
        if !self.operations.frozen()
            && let Some(draft) = &self.draft
        {
            self.forms.sync(&draft.value, cx);
        }
        self.set_palette(
            Palette::new(&settings.accent_color, &settings.background_color),
            cx,
        );
        if saved {
            self.save_error = None;
            cx.emit(SettingsEvent::Saved(settings));
            self.saved_visible = true;
            self.saved_epoch += 1;
            let epoch = self.saved_epoch;
            cx.spawn(async move |panel, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(1800))
                    .await;
                let _ = panel.update(cx, |panel, cx| {
                    if panel.saved_epoch == epoch {
                        panel.saved_visible = false;
                        cx.notify();
                    }
                });
            })
            .detach();
        }
    }
    pub(super) fn stage(&mut self, patch: SettingsPatch, cx: &mut Context<Self>) {
        if self.operations.frozen() {
            return;
        }
        let Some(draft) = &mut self.draft else {
            return;
        };
        match draft.stage(patch) {
            Ok(true) => {
                self.save_error = None;
                self.saved_visible = false;
                self.save_ready = false;
                let revision = draft.revision;
                cx.spawn(async move |panel, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(400))
                        .await;
                    let _ = panel.update(cx, |panel, cx| {
                        if panel
                            .draft
                            .as_ref()
                            .is_some_and(|draft| draft.revision == revision)
                        {
                            panel.save_ready = true;
                            panel.begin_save(cx);
                        }
                    });
                })
                .detach();
            }
            Ok(false) => {}
            Err(error) => draft.error = Some(error.to_string()),
        }
        cx.notify();
    }
    pub(super) fn begin_save(&mut self, cx: &mut Context<Self>) {
        if self.operations.frozen()
            || self.loading
            || self.environment_writing
            || self
                .draft
                .as_ref()
                .is_some_and(|draft| draft.inflight.is_some())
        {
            return;
        }
        if let Some(write) = self.environment_write.take() {
            self.send_environment(write, cx);
            return;
        }
        if !self.save_ready || !self.forms.errors.is_empty() {
            return;
        }
        let Some(patch) = self.draft.as_mut().and_then(state::Draft::take_save) else {
            return;
        };
        self.save_ready = false;
        let error_field = errors::Field::from_patch(&patch);
        let client = self.client.clone();
        cx.spawn(async move |panel, cx| {
            let result = client
                .execute(NativeAction::Settings(
                    shellfox_host::SettingsRequest::Save(Box::new(patch)),
                ))
                .await;
            let _ = panel.update(cx, |panel, cx| {
                match result {
                    Ok(NativeResponse::Settings(settings)) => {
                        panel.accept_settings(settings, true, cx);
                    }
                    Ok(_) => {
                        if let Some(draft) = &mut panel.draft {
                            draft.failed("Invalid settings response from host.".into());
                        }
                    }
                    Err(error) => {
                        if let Some(draft) = &mut panel.draft {
                            draft.failed(error.to_string());
                        }
                    }
                }
                if let Some(message) = panel.draft.as_ref().and_then(|draft| draft.error.clone()) {
                    panel.save_error = Some((error_field, message));
                }
                panel.begin_save(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn status(&self) -> &'static str {
        if !self.forms.errors.is_empty() {
            return "Check settings";
        }
        if let Some(draft) = &self.draft {
            if draft.inflight.is_some() {
                return "Saving…";
            }
            if draft.dirty() {
                return if draft.error.is_some() || !self.forms.errors.is_empty() {
                    "Check settings"
                } else {
                    "Unsaved changes"
                };
            }
        }
        if self.saved_visible { "Saved" } else { "" }
    }
}
