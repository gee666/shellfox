//! Settings validation only. Catalog reads construct commands but never launch
//! shells, Python, UAC or arbitrary executables. Installation belongs to the owner.
use shellfox_core::{Error, ErrorCode, Result, Settings, SettingsShell};
use shellfox_host::{normalize_settings_profile, SettingsProfile, SettingsRuntime};
use shellfox_protocol::CancellationToken;
use shellfox_terminal::{
    default_shell_profile, discover_shell_profiles, shell_profile, ShellProfile, ShellProfileId,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub const SETTINGS_PROBE_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPythonTarget<'a> {
    LinuxLocal,
    Wsl { profile_id: &'a str },
}
/// An injected provider must use a scoped owned probe, honor this deadline and
/// token, and confirm descendant cleanup before returning on ANY result. It must
/// not call Host, spawn an uncontained child, or persist probe output/credentials.
pub struct SettingsPythonRequest<'a> {
    pub configured: Option<&'a str>,
    pub target: SettingsPythonTarget<'a>,
    pub deadline: Instant,
    pub ownership_lost: &'a CancellationToken,
}
impl SettingsPythonRequest<'_> {
    pub fn check_owned(&self) -> Result<()> {
        check(self.ownership_lost, self.deadline)
    }
}
pub trait SettingsPythonValidation: Send + Sync {
    fn validate(&self, request: SettingsPythonRequest<'_>) -> Result<()>;
}
/// Explicit native gate until the host has an owned Python probe provider.
/// Windows local PowerShell/Cmd settings never invoke this provider.
pub struct UnavailableSettingsPythonValidation;
impl SettingsPythonValidation for UnavailableSettingsPythonValidation {
    fn validate(&self, request: SettingsPythonRequest<'_>) -> Result<()> {
        request.check_owned()?;
        Err(Error::new(
            ErrorCode::Unsupported,
            "An owned Python validation provider is unavailable.",
        ))
    }
}

pub struct SettingsRuntimeAdapter {
    lost: CancellationToken,
    python: Arc<dyn SettingsPythonValidation>,
    probe_timeout: Duration,
}
impl SettingsRuntimeAdapter {
    /// Pass the FINAL native ownership token, after the service receives it and
    /// before readiness is published. Do not install against a temporary token.
    pub fn new(lost: CancellationToken, python: Arc<dyn SettingsPythonValidation>) -> Self {
        Self {
            lost,
            python,
            probe_timeout: SETTINGS_PROBE_TIMEOUT,
        }
    }
    pub fn with_probe_timeout(mut self, timeout: Duration) -> Result<Self> {
        shellfox_core::validation::ensure(
            !timeout.is_zero() && timeout <= SETTINGS_PROBE_TIMEOUT,
            "Invalid settings probe timeout.",
        )?;
        self.probe_timeout = timeout;
        Ok(self)
    }
    fn python(
        &self,
        candidate: &Settings,
        target: SettingsPythonTarget<'_>,
        deadline: Instant,
    ) -> Result<()> {
        check(&self.lost, deadline)?;
        let result = self.python.validate(SettingsPythonRequest {
            configured: candidate.python_path.as_deref(),
            target,
            deadline,
            ownership_lost: &self.lost,
        });
        // Cancellation/deadline wins even when a provider returns success/error.
        check(&self.lost, deadline)?;
        result.map_err(|error| {
            Error::new(
                if error.code == ErrorCode::Unsupported {
                    ErrorCode::Unsupported
                } else {
                    ErrorCode::Validation
                },
                "Python validation did not confirm safe process ownership.",
            )
        })
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
        if cfg!(target_os = "linux") && candidate.python_path != previous.python_path {
            self.python(&candidate, SettingsPythonTarget::LinuxLocal, deadline)?;
        }
        check(&self.lost, deadline)?;
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
            self.python(
                &candidate,
                SettingsPythonTarget::Wsl {
                    profile_id: selected.id().as_str(),
                },
                deadline,
            )?;
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
            ShellProfileId::Cmd | ShellProfileId::LoginShell => SettingsShell::LoginShell,
        },
        executable: profile.executable()?.to_str()?.to_owned(),
        available: profile.unavailable_reason().is_none(),
        elevation: false,
    })
}
