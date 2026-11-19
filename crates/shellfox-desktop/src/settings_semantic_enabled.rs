use super::*;
impl SettingsPanel {
    pub(super) fn control_available(&self, action: &controls::Action, cx: &App) -> bool {
        if self.operations.frozen() {
            return false;
        }
        use controls::Action;
        match action {
            Action::AddAgent => {
                self.draft.is_some()
                    && !self.forms.fields[&Input::Agent]
                        .read(cx)
                        .text
                        .trim()
                        .is_empty()
            }
            Action::Profiles => self.shell_profiles().next().is_some(),
            Action::SelectProfile(id) => self
                .shell_profiles()
                .any(|profile| profile.id == *id && profile.available),
            Action::Ssh(operation) => {
                if self.ssh.pending {
                    return false;
                }
                match operation {
                    ssh::Operation::Import => {
                        self.ssh.profiles.is_some()
                            && self.ssh.editor.is_none()
                            && self.ssh.removing.is_none()
                    }
                    ssh::Operation::Cancel => true,
                    _ => self.ssh.profiles.is_some(),
                }
            }
            Action::Remote(operation) => match operation {
                operation if operation.client_access_target().is_some() => {
                    !self.remote.pending && self.remote.client_access_name(operation).is_some()
                }
                remote::Operation::Advanced
                | remote::Operation::OpenLogin
                | remote::Operation::CancelRevoke => true,
                remote::Operation::CancelLogin => {
                    self.remote.login.busy() && (!self.remote.pending || self.remote.login_request)
                }
                remote::Operation::CancelAttempt(id) | remote::Operation::ContinueAttempt(id) => {
                    !self.remote.pending
                        && self.remote.state.as_ref().is_some_and(|state| {
                            state.attempts.iter().any(|attempt| attempt.id == *id)
                        })
                }
                remote::Operation::ToggleEnabled => {
                    self.remote.state.is_some() && !self.remote.pending && !self.remote.login.busy()
                }
                remote::Operation::SaveHostname => {
                    let hostname = self.remote.hostname.read(cx).text.trim();
                    self.remote.state.as_ref().is_some_and(|state| {
                        !self.remote.pending && !hostname.is_empty() && hostname != state.hostname
                    })
                }
                remote::Operation::Login => {
                    !self.remote.pending
                        && !self.remote.login.busy()
                        && (self.remote.retry_release_id().is_some()
                            || self
                                .remote
                                .state
                                .as_ref()
                                .is_some_and(|state| !login::blocked(state)))
                }
                remote::Operation::UseKey => {
                    self.remote
                        .state
                        .as_ref()
                        .is_some_and(|state| !login::blocked(state))
                        && !self.remote.pending
                        && !self.remote.login.busy()
                        && !self.remote.auth.read(cx).text.trim().is_empty()
                }
                remote::Operation::Connect(id) => {
                    !self.remote.pending
                        && self.remote.state.as_ref().is_some_and(|state| {
                            state.peers.iter().any(|peer| {
                                peer.id == *id
                                    && peer.online
                                    && matches!(
                                        peer.connection,
                                        api::Connection::Disconnected | api::Connection::Rejected
                                    )
                            })
                        })
                }
                remote::Operation::Disconnect(id) => {
                    !self.remote.pending || self.remote.inflight_peer.as_ref() == Some(id)
                }
                _ => !self.remote.pending,
            },
            _ => true,
        }
    }
}
