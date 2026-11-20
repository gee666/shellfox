//! Settings validation only. Catalog reads never launch executables.
use shellfox_core::{Error, ErrorCode, Result, Settings, SettingsShell};
use shellfox_host::{SettingsProfile, SettingsRuntime, normalize_settings_profile};
use shellfox_protocol::CancellationToken;
use shellfox_terminal::{
    ShellProfile, ShellProfileId, default_shell_profile, discover_shell_profiles, shell_profile,
};
use std::time::{Duration, Instant};

pub const SETTINGS_PROBE_TIMEOUT: Duration = Duration::from_secs(8);

pub struct SettingsRuntimeAdapter {
    lost: CancellationToken,
    probe_timeout: Duration,
    #[cfg(windows)]
    wsl_catalog: crate::windows_profiles::WslCatalogCache,
}
impl SettingsRuntimeAdapter {
    /// Pass the FINAL native ownership token, after the service receives it and
    /// before readiness is published. Do not install against a temporary token.
    pub fn new(lost: CancellationToken) -> Self {
        Self {
            lost,
            probe_timeout: SETTINGS_PROBE_TIMEOUT,
            #[cfg(windows)]
            wsl_catalog: Default::default(),
        }
    }
    #[cfg(windows)]
    pub(crate) fn with_wsl_catalog(
        mut self,
        catalog: crate::windows_profiles::WslCatalogCache,
    ) -> Self {
        self.wsl_catalog = catalog;
        self
    }

    pub fn with_probe_timeout(mut self, timeout: Duration) -> Result<Self> {
        shellfox_core::validation::ensure(
            !timeout.is_zero() && timeout <= SETTINGS_PROBE_TIMEOUT,
            "Invalid settings probe timeout.",
        )?;
        self.probe_timeout = timeout;
        Ok(self)
    }
    fn normalize_owned(
        &self,
        previous: &Settings,
        candidate: Settings,
        deadline: Instant,
    ) -> Result<Settings> {
        candidate.validate()?;
        let selection_changed = candidate.terminal_profile_id != previous.terminal_profile_id
            || candidate.shell_id != previous.shell_id
            || candidate.shell_executable != previous.shell_executable
            || candidate.adapter_id != previous.adapter_id;
        check(&self.lost, deadline)?;
        #[cfg(windows)]
        if let Some(id) = candidate
            .terminal_profile_id
            .as_ref()
            .and_then(Option::as_deref)
            .filter(|id| id.starts_with("wsl:"))
        {
            let selected = self
                .wsl_catalog
                .ready()
                .and_then(|profiles| profiles.settings_profile(id));
            return match selected {
                Ok(selected) => {
                    let id = selected.id.clone();
                    normalize_settings_profile(previous, candidate, &[selected], Some(&id))
                }
                Err(_) if !selection_changed => Ok(candidate),
                Err(_) => Err(unavailable_profile()),
            };
        }
        let catalog = discover_shell_profiles().map_err(|_| unavailable_catalog())?;
        check(&self.lost, deadline)?;
        let requested = requested_profile(&candidate, &catalog);
        check(&self.lost, deadline)?;
        let selected =
            match requested.and_then(|id| shell_profile(id).map_err(|_| unavailable_profile())) {
                Ok(selected) => selected,
                // A saved unavailable/opaque profile remains configuration, never a
                // request to replace it with an available shell during an autosave.
                Err(_) if !selection_changed => return Ok(candidate),
                Err(error) => return Err(error),
            };
        check(&self.lost, deadline)?;
        if selected.id() == ShellProfileId::Wsl && selection_changed {
            // WSL profile changes must come from the owned per-distro catalog above.
            return Err(unavailable_profile());
        }
        let mut profiles: Vec<_> = catalog.iter().filter_map(settings_profile).collect();
        // shell_profile rechecks the executable using CURRENT shared discovery.
        // Prefer that evidence if the file changed between the two catalog reads.
        profiles.retain(|profile| profile.id != selected.id().as_str());
        profiles.push(settings_profile(&selected).ok_or_else(unavailable_profile)?);
        normalize_settings_profile(previous, candidate, &profiles, Some(selected.id().as_str()))
    }
}
impl SettingsRuntime for SettingsRuntimeAdapter {
    fn check_commit(&self) -> Result<()> {
        check_ownership(&self.lost)
    }
    fn normalize(&self, previous: &Settings, candidate: Settings) -> Result<Settings> {
        let deadline = Instant::now() + self.probe_timeout;
        check(&self.lost, deadline)?;
        let result = self.normalize_owned(previous, candidate, deadline);
        // Host invokes this callback inside its transaction. No successful return
        // after lost ownership or an expired cooperative probe budget may commit.
        check(&self.lost, deadline)?;
        result
    }
}
fn check_ownership(lost: &CancellationToken) -> Result<()> {
    if lost.is_cancelled() {
        return Err(Error::new(
            ErrorCode::Internal,
            "Native host ownership was lost.",
        ));
    }
    Ok(())
}
fn check(lost: &CancellationToken, deadline: Instant) -> Result<()> {
    check_ownership(lost)?;
    if Instant::now() >= deadline {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "Settings validation exceeded its owned probe deadline.",
        ));
    }
    Ok(())
}
fn unavailable_catalog() -> Error {
    Error::new(
        ErrorCode::Unsupported,
        "The current native shell catalog is unavailable.",
    )
}
fn unavailable_profile() -> Error {
    Error::new(
        ErrorCode::Unsupported,
        "The requested profile has no supported native process ownership.",
    )
}
fn requested_profile(settings: &Settings, catalog: &[ShellProfile]) -> Result<ShellProfileId> {
    if let Some(Some(explicit)) = &settings.terminal_profile_id {
        return explicit.parse().map_err(|_| unavailable_profile());
    }
    let id = match settings.shell_id {
        SettingsShell::Pwsh => "pwsh",
        SettingsShell::WindowsPowershell => "windows-powershell",
        SettingsShell::Bash => "bash",
        SettingsShell::LoginShell => "login-shell",
        SettingsShell::Wsl => "wsl",
    };
    if let Some(profile) = catalog.iter().find(|profile| {
        profile.id().as_str() == id
            && settings.shell_executable.as_ref().is_none_or(|exe| {
                profile.executable().and_then(|p| p.to_str()) == Some(exe.as_str())
            })
    }) {
        return Ok(profile.id());
    }
    default_shell_profile()
        .map(|profile| profile.id())
        .map_err(|_| unavailable_profile())
}
fn settings_profile(profile: &ShellProfile) -> Option<SettingsProfile> {
    Some(SettingsProfile {
        id: profile.id().as_str().into(),
        // Electron's service maps any non-WSL/non-PowerShell local profile,
        // including Cmd, to login-shell. There is no wire SettingsShell::Cmd.
        shell_id: match profile.id() {
            ShellProfileId::Pwsh => SettingsShell::Pwsh,
            ShellProfileId::WindowsPowerShell => SettingsShell::WindowsPowershell,
            ShellProfileId::Wsl => SettingsShell::Wsl,
            ShellProfileId::Bash => SettingsShell::Bash,
            ShellProfileId::Cmd
            | ShellProfileId::LoginShell
            | ShellProfileId::Sh
            | ShellProfileId::Zsh
            | ShellProfileId::Fish => SettingsShell::LoginShell,
        },
        executable: profile.executable()?.to_str()?.to_owned(),
        available: profile.unavailable_reason().is_none(),
        elevation: false,
    })
}
