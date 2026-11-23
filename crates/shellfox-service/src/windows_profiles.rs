use anyhow::{Context, Result, ensure};
use shellfox_core::{SettingsShell, WslLaunchAccount, WslLaunchBinding};
use shellfox_host::SettingsProfile;
use shellfox_protocol::{Elevation, ProfileEnvironment, TerminalProfile};
use shellfox_terminal::{PtyCommand, PtyConfig, elevated::InteractiveHelper, wsl::{OsWsl, WslProfileId}};

mod cache;
pub(crate) use cache::{WslCatalogCache, WslCatalogWorker};

pub(crate) const WSL_PREFLIGHT_FAILED: &str = "Requires accessible Bash, env, Python3, /proc and working Linux pidfd signal support. Guest preflight failed; no shell may be launched.";

pub(crate) fn administrator_profile(
    powershell: &TerminalProfile,
    backend: bool,
) -> TerminalProfile {
    let reason = if !backend {
        Some("The native terminal backend is unavailable.")
    } else if InteractiveHelper::bundled()
        .and_then(|helper| helper.probe())
        .is_err()
    {
        Some(
            "The bundled administrator ConPTY helper is unavailable. Requires Windows 10 1809 or newer.",
        )
    } else {
        None
    };
    TerminalProfile {
        id: super::ADMIN_POWERSHELL_PROFILE.into(),
        label: "Administrator Windows PowerShell".into(),
        environment: ProfileEnvironment::Local,
        executable: powershell.executable.clone(),
        args: vec!["-NoLogo".into()],
        distro: None,
        available: reason.is_none(),
        unavailable_reason: Some(reason.map(str::to_owned)),
        can_terminate_descendants: Some(reason.is_none()),
        elevation: Some(Elevation::Administrator),
    }
}

pub(crate) struct WslProfiles {
    wsl: OsWsl,
    distros: Vec<(String, bool)>,
}
impl WslProfiles {
    fn discover(cancelled: impl Fn() -> bool + Sync) -> Result<Self> {
        let wsl = OsWsl::discover()?;
        let distros = wsl.probe_interactive_profiles(cancelled)?;
        Ok(Self { wsl, distros })
    }

    fn args(distro: &str) -> [String; 2] {
        ["-d".into(), distro.into()]
    }

    fn select(&self, id: &str) -> Result<&str> {
        let name = id.strip_prefix("wsl:").context("Invalid WSL profile ID.")?;
        let (distro, available) = self
            .distros
            .iter()
            .find(|(distro, _)| distro == name)
            .context("The selected WSL distribution is not installed.")?;
        ensure!(*available, WSL_PREFLIGHT_FAILED);
        Ok(distro)
    }

    pub(crate) fn wire_profiles(
        &self,
        backend: bool,
    ) -> impl Iterator<Item = TerminalProfile> + '_ {
        self.distros
            .iter()
            .map(move |(distro, eligible)| TerminalProfile {
                id: format!("wsl:{distro}"),
                label: format!("WSL · {distro}"),
                environment: ProfileEnvironment::Wsl,
                executable: self.wsl.path().to_string_lossy().into_owned(),
                args: Self::args(distro).into(),
                distro: Some(distro.clone()),
                available: backend && *eligible,
                unavailable_reason: Some(if !eligible {
                    Some(WSL_PREFLIGHT_FAILED.into())
                } else {
                    (!backend).then(|| "The native terminal backend is unavailable.".into())
                }),
                // The Windows PTY owns wsl.exe, not guest descendants.
                can_terminate_descendants: Some(false),
                elevation: None,
            })
    }

    pub(crate) fn settings_profile(&self, id: &str) -> Result<SettingsProfile> {
        self.select(id)?;
        Ok(SettingsProfile {
            id: id.into(),
            shell_id: SettingsShell::Wsl,
            executable: self.wsl.path().to_string_lossy().into_owned(),
            available: true,
            elevation: false,
        })
    }

    pub(crate) fn configure(&self, id: &str, config: &mut PtyConfig) -> Result<()> {
        let distro = self.select(id)?;
        let args = Self::args(distro).into_iter().map(Into::into).collect();
        if let Some((name, _)) = wsl_unc(config.cwd.to_str().context("Invalid WSL directory")?)
        {
            ensure!(
                name.eq_ignore_ascii_case(distro),
                "The folder belongs to a different WSL distribution."
            );
            // The guest validates this cwd as the pinned account. The Windows
            // UNC server may already be using a different default user, so
            // wsl.exe starts in the user's profile folder, or else the host's
            // own directory as in 0.9 (which may be the install directory).
            config.cwd = std::env::var_os("USERPROFILE")
                .map(std::path::PathBuf::from)
                .into_iter()
                .chain(std::env::current_dir().ok())
                .find(|dir| dir.is_absolute() && dir.is_dir())
                .context("No Windows directory to start WSL from.")?;
        }
        config.command = Some(PtyCommand {
            program: self.wsl.path().into(),
            args,
        });
        Ok(())
    }

    pub(crate) fn pin_launch_account(
        &self,
        id: &str,
        binding: Option<&WslLaunchBinding>,
        cwd: &str,
        config: &mut PtyConfig,
    ) -> Result<Option<WslLaunchAccount>> {
        let distro = self.select(id)?;
        let observed;
        let account = match binding {
            Some(WslLaunchBinding::Bound { account }) => account,
            _ => {
                let bytes = self.wsl.launch_account(&WslProfileId::parse(id)?)
                    .context("Could not verify the default Linux user for this WSL launch.")?;
                observed = serde_json::from_slice::<WslLaunchAccount>(&bytes)
                    .context("WSL requires a verified ordinary default user. A root account is not supported.")?;
                &observed
            }
        };
        // WSL's --user accepts a name, not a numeric UID. The guest bootstrap verifies
        // the resolved account before exec, even if the default or passwd entry changed.
        let command = config.command.as_mut().context("WSL launch command was not prepared")?;
        let guest = wsl_unc(cwd).map(|(_, path)| path);
        command.args.extend(["--user".into(), account.name().into()]);
        if guest.is_some() {
            command.args.extend(["--cd".into(), "/".into()]);
        }
        command.args.extend([
            "--exec".into(),
            "/usr/bin/python3".into(), "-I".into(), "-S".into(), "-c".into(),
            include_str!("../../../native/wsl/launch-shell.py").into(),
            distro.into(), account.uid().to_string().into(), account.name().into(),
            guest.unwrap_or_default().into(),
        ]);
        Ok(matches!(binding, Some(WslLaunchBinding::Pending)).then(|| account.clone()))
    }
}

fn wsl_unc(path: &str) -> Option<(&str, String)> {
    let mut parts = path.strip_prefix(r"\\")?.splitn(3, '\\');
    let server = parts.next()?;
    if !server.eq_ignore_ascii_case("wsl.localhost") && !server.eq_ignore_ascii_case("wsl$") {
        return None;
    }
    Some((
        parts.next()?,
        format!("/{}", parts.next().unwrap_or_default().replace('\\', "/")),
    ))
}
