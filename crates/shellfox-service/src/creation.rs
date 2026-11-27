use crate::*;
use shellfox_core::{CreateSession, SettingsAdapter, SettingsShell};
use shellfox_terminal::ShellProfileId;
#[cfg(test)]
mod tests;

impl Service {
    /// [`crate::folder_profile`] over the trusted native catalog that
    /// [`Self::create_application_session`] accepts.
    pub(crate) fn folder_launch_profile(
        &self,
        saved: &shellfox_core::ProfileId,
        cwd: &str,
    ) -> Result<shellfox_core::ProfileId> {
        let local = shellfox_terminal::discover_shell_profiles()?;
        #[cfg(windows)]
        let wsl: Vec<_> = self
            .catalog
            .wsl
            .cache
            .ready()
            .map(|wsl| wsl.wire_profiles(true).collect())
            .unwrap_or_default();
        #[cfg(not(windows))]
        let wsl: Vec<shellfox_protocol::TerminalProfile> = Vec::new();
        let candidates: Vec<_> = local
            .iter()
            .map(|p| crate::LaunchCandidate {
                id: p.id().as_str(),
                available: p.unavailable_reason().is_none(),
                elevated: false,
            })
            .chain(wsl.iter().map(Into::into))
            .collect();
        crate::folder_profile(&candidates, saved.as_str(), cwd, cfg!(windows))
            .context("No terminal profile is available for this folder.")?
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
            let profile = self
                .catalog
                .wsl
                .cache
                .ready()?
                .settings_profile(input.profile_id.as_str())?;
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

    fn create_profile_session(
        &self,
        input: CreateSession,
        shell_id: SettingsShell,
    ) -> Result<Snapshot> {
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
                let tab = snapshot
                    .selection
                    .tab_id
                    .context("New administrator tab missing.")?;
                self.start_administrator_tab(tab)?;
                Ok(snapshot)
            }
            #[cfg(windows)]
            shellfox_host::HostRequest::AddTab { session_id, input }
                if input.profile_id.as_str().starts_with("wsl:") =>
            {
                self.assert_accepting()?;
                self.catalog
                    .wsl
                    .cache
                    .ready()?
                    .settings_profile(input.profile_id.as_str())?;
                self.host.add_tab(session_id, input).map_err(Into::into)
            }
            request => {
                let selected = match &request {
                    shellfox_host::HostRequest::ActivateSession { session_id } => Some(*session_id),
                    _ => None,
                };
                let snapshot = self.host.request(request)?;
                if let Some(id) = selected {
                    self.touch_session(id);
                }
                Ok(snapshot)
            }
        }
    }
}
