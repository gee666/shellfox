use super::*;
pub(super) async fn execute_open(
    gateway: &dyn native_gateway::Gateway,
    input: shellfox_service::ssh::OpenProfile,
) -> Result<shellfox_service::ssh::OpenedProfile, String> {
    let action = NativeAction::SshOpen(input);
    action.validate().map_err(|error| error.to_string())?;
    match gateway.execute(action).await {
        Ok(NativeResponse::SshOpened(opened)) => Ok(opened),
        Ok(_) => Err("Invalid typed SSH opening response from the shared host.".into()),
        Err(error) => Err(error.to_string()),
    }
}
impl SettingsPanel {
    /// Local queue admission only. Root retains the ORIGINAL request ID and
    /// subscribes to success AND failure on this window before calling.
    /// This path deliberately uses this panel's exact HostClient, not a fork or
    /// a replacement auxiliary provider with a different controller scope.
    pub fn open_ssh_profile(
        &mut self,
        input: shellfox_service::ssh::OpenProfile,
        cx: &mut Context<Self>,
    ) -> SshOpenAdmission {
        let request_id = input.request_id;
        let admission =
            self.ssh_open_intent
                .admit(request_id, self.operations.frozen(), self.ssh.pending);
        if !matches!(admission, SshOpenAdmission::Queued { .. }) {
            return admission;
        }
        self.ssh.pending = true;
        self.ssh.error = None;
        let client = self.client.clone();
        let lease = self.operations.begin();
        cx.spawn(async move |panel, cx| {
            let result = execute_open(&client, input).await;
            let _ = panel.update(cx, |panel, cx| {
                drop(lease);
                let Some(completion) = panel.ssh_open_intent.finish(request_id, result) else {
                    return;
                };
                panel.ssh.pending = false;
                match completion.result {
                    Ok(opened) => {
                        panel.ssh.error = None;
                        cx.emit(SettingsSshOpened {
                            request_id: completion.request_id,
                            client: client.clone(),
                            opened,
                        });
                    }
                    Err(message) => {
                        panel.ssh.error = Some(message.clone());
                        cx.emit(SettingsSshOpenFailed {
                            request_id: completion.request_id,
                            message,
                        });
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
        admission
    }
}
