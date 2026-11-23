use crate::*;
use shellfox_core::{CreateSession, SettingsAdapter, SettingsShell};
use shellfox_terminal::ShellProfileId;
#[cfg(test)]
mod tests;

impl Service {
    /// [`crate::effective_default_profile`] over the trusted native catalog that
    /// [`Self::create_application_session`] accepts.
    pub(crate) fn default_launch_profile(&self, saved: &shellfox_core::ProfileId) -> Result<shellfox_core::ProfileId> {
        #[cfg(windows)]
        if saved.as_str().starts_with("wsl:")
            && self.catalog.wsl.cache.ready().is_ok_and(|wsl| wsl.settings_profile(saved.as_str()).is_ok())
        {
            return Ok(saved.clone());
        }
        let local = shellfox_terminal::discover_shell_profiles()?;
        let usable = local.iter().filter(|p| p.unavailable_reason().is_none()).map(|p| p.id().as_str());
        crate::effective_default_profile(saved.as_str(), usable)
            .context("No terminal profile is available on this host.")?
            .parse()
            .map_err(Into::into)
    }
    /// Application intent creation captures its selected trusted catalog entry.
    /// Availability/process ownership remain separate: these facts authorize no
    /// launch, PID lookup, or claim that a shell is currently alive.
    pub(crate) fn create_application_session(&self, input: CreateSession) -> Result<Snapshot> {
        self.assert_accepting()?;
        #[cfg(windows)]
        if input.profile_id.as_str().starts_with("wsl:") {
            let profile = self.catalog.wsl.cache.ready()?.settings_profile(input.profile_id.as_str())?;
            return self.create_profile_session(input, profile.shell_id);
        }
        let profile = shellfox_terminal::discover_shell_profiles()?
            .into_iter()
            .find(|profile| profile.id().as_str() == input.profile_id.as_str())
            .context("Requested session profile is not in the trusted native catalog.")?;
        let shell_id = match profile.id() {
            ShellProfileId::Pwsh => SettingsShell::Pwsh,
            ShellProfileId::WindowsPowerShell => SettingsShell::WindowsPowershell,
            ShellProfileId::Wsl => SettingsShell::Wsl,
            ShellProfileId::Bash => SettingsShell::Bash,
            ShellProfileId::Cmd
            | ShellProfileId::LoginShell
            | ShellProfileId::Sh
            | ShellProfileId::Zsh
            | ShellProfileId::Fish => SettingsShell::LoginShell,
        };
        self.create_profile_session(input, shell_id)
    }

    fn create_profile_session(&self, input: CreateSession, shell_id: SettingsShell) -> Result<Snapshot> {
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
        &mut self,
        request: shellfox_host::HostRequest,
    ) -> Result<Snapshot> {
        match request {
            shellfox_host::HostRequest::CreateSession(input) => {
                self.create_application_session(input)
            }
            #[cfg(windows)]
            shellfox_host::HostRequest::AddTab { session_id, input }
                if input.profile_id.as_str() == ADMIN_POWERSHELL_PROFILE =>
            {
                self.assert_accepting()?;
                // Only this fresh local action may request UAC. Neither saved
                // metadata, default settings nor a subsequent attach can do so.
                shellfox_terminal::elevated::InteractiveHelper::bundled()?;
                let snapshot = self.host.add_tab(session_id, input)?;
                let tab = snapshot.selection.tab_id.context("New administrator tab missing.")?;
                self.start_administrator_tab(tab)?;
                Ok(snapshot)
            }
            #[cfg(windows)]
            shellfox_host::HostRequest::AddTab { session_id, input }
                if input.profile_id.as_str().starts_with("wsl:") =>
            {
                self.assert_accepting()?;
                self.catalog.wsl.cache.ready()?.settings_profile(input.profile_id.as_str())?;
                self.host.add_tab(session_id, input).map_err(Into::into)
            }
            request => self.host.request(request).map_err(Into::into),
        }
    }
}
