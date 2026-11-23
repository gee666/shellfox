use super::*;
pub(super) struct PendingEnvironment {
    pub identity: gpui::EntityId,
    pub env: Vec<shellfox_core::EnvVar>,
}
impl SettingsPanel {
    pub(super) fn send_environment(&mut self, write: PendingEnvironment, cx: &mut Context<Self>) {
        self.environment_writing = true;
        let client = self.client.clone();
        cx.spawn(async move |panel, cx| {
            let result = client
                .execute(NativeAction::Settings(
                    shellfox_host::SettingsRequest::Save(Box::new(SettingsPatch {
                        terminal_env: Some(write.env),
                        ..Default::default()
                    })),
                ))
                .await;
            let _ = panel.update(cx, |panel, cx| {
                panel.environment_writing = false;
                let current = panel
                    .environment
                    .as_ref()
                    .is_some_and(|environment| environment.text.entity_id() == write.identity);
                match result {
                    Ok(NativeResponse::Settings(settings)) if settings.validate().is_ok() => {
                        panel.accept_settings(settings.clone(), false, cx);
                        cx.emit(SettingsEvent::Saved(settings));
                        if current {
                            panel.environment = None;
                            panel.environment_subscription = None;
                            panel.restore_environment_focus = true;
                        }
                    }
                    error => {
                        let message = match error {
                            Err(error) => error.to_string(),
                            _ => "Invalid settings response from host.".into(),
                        };
                        if current {
                            if let Some(environment) = &mut panel.environment {
                                environment.pending = false;
                                environment.error = Some(message);
                                environment
                                    .text
                                    .update(cx, |field, cx| field.set_read_only(false, cx));
                            }
                        } else {
                            panel.auxiliary_error = Some(message);
                        }
                    }
                }
                panel.begin_save(cx);
                cx.notify();
            });
        })
        .detach();
    }
}
