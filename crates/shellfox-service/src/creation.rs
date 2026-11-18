use crate::*;
use shellfox_core::{CreateSession, SettingsAdapter, SettingsShell};
use shellfox_terminal::ShellProfileId;
#[cfg(test)]
#[path = "creation_tests.rs"]
mod tests;

impl Service {
    /// Application intent creation captures its selected trusted catalog entry.
    /// Availability/process ownership remain separate: these facts authorize no
    /// launch, PID lookup, or claim that a shell is currently alive.
    pub(crate) fn create_application_session(&self, input: CreateSession) -> Result<Snapshot> {
        self.assert_accepting()?;
        let profile = shellfox_terminal::discover_shell_profiles()?
            .into_iter()
            .find(|profile| profile.id().as_str() == input.profile_id.as_str())
            .context("Requested session profile is not in the trusted native catalog.")?;
        let shell_id = match profile.id() {
            ShellProfileId::Pwsh => SettingsShell::Pwsh,
            ShellProfileId::WindowsPowerShell => SettingsShell::WindowsPowershell,
            ShellProfileId::Wsl => SettingsShell::Wsl,
            ShellProfileId::Cmd | ShellProfileId::LoginShell => SettingsShell::LoginShell,
        };
        self.host
            .create_session_with_facts(
                input,
                shellfox_host::SessionCreationFacts {
                    version: 1,
                    adapter_id: SettingsAdapter::EmbeddedPty,
                    shell_id,
                },
            )
            .map_err(Into::into)
    }
    pub(crate) fn application_metadata(
        &self,
        request: shellfox_host::HostRequest,
    ) -> Result<Snapshot> {
        match request {
            shellfox_host::HostRequest::CreateSession(input) => {
                self.create_application_session(input)
            }
            request => self.host.request(request).map_err(Into::into),
        }
    }
}
