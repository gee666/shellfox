//! Local shell choices. Discovery never executes a shell or reads app settings.
use crate::PtyCommand;
use anyhow::{bail, Context, Result};
use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellProfileId {
    Pwsh,
    WindowsPowerShell,
    Cmd,
    Wsl,
    LoginShell,
    Bash,
    Sh,
    Zsh,
    Fish,
}

impl ShellProfileId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pwsh => "pwsh",
            Self::WindowsPowerShell => "windows-powershell",
            Self::Cmd => "cmd",
            Self::Wsl => "wsl",
            Self::LoginShell => "login-shell",
            Self::Bash => "bash",
            Self::Sh => "sh",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
        }
    }
}

impl FromStr for ShellProfileId {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "pwsh" => Ok(Self::Pwsh),
            "windows-powershell" => Ok(Self::WindowsPowerShell),
            "cmd" => Ok(Self::Cmd),
            "wsl" => Ok(Self::Wsl),
            "login-shell" => Ok(Self::LoginShell),
            "bash" => Ok(Self::Bash),
            "sh" => Ok(Self::Sh),
            "zsh" => Ok(Self::Zsh),
            "fish" => Ok(Self::Fish),
            _ => bail!("unknown shell profile {value:?}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ShellProfile {
    id: ShellProfileId,
    label: &'static str,
    executable: Option<PathBuf>,
    unavailable_reason: Option<&'static str>,
}

impl ShellProfile {
    pub fn id(&self) -> ShellProfileId {
        self.id
    }
    pub fn label(&self) -> &str {
        self.label
    }
    pub fn executable(&self) -> Option<&Path> {
        self.executable.as_deref()
    }
    pub fn unavailable_reason(&self) -> Option<&str> {
        self.unavailable_reason
    }
    pub fn command(&self) -> Result<PtyCommand> {
        if let Some(reason) = self.unavailable_reason {
            bail!("{} unavailable: {reason}", self.label);
        }
        let program = self
            .executable
            .as_ref()
            .context("missing shell executable")?;
        anyhow::ensure!(
            program.is_absolute() && program.is_file(),
            "shell executable disappeared: {}",
            program.display()
        );
        let args: &[&str] = match self.id {
            ShellProfileId::Pwsh | ShellProfileId::WindowsPowerShell => &[
                "-NoLogo",
                "-NoProfile",
                "-NoExit",
                "-Command",
                include_str!("powershell-init.ps1"),
            ],
            ShellProfileId::Cmd => &["/D", "/Q"],
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            ShellProfileId::LoginShell
            | ShellProfileId::Bash
            | ShellProfileId::Sh
            | ShellProfileId::Zsh
            | ShellProfileId::Fish => {
                anyhow::ensure!(
                    unix_executable(program),
                    "shell is not executable: {}",
                    program.display()
                );
                match program.file_name().and_then(|s| s.to_str()) {
                    Some("bash") => &["--noprofile", "--norc", "-i"],
                    Some("zsh") => &["-f", "-i"],
                    Some("fish") => &["--no-config", "-i"],
                    _ => &["-i"],
                }
            }
            _ => bail!("shell has no safe local supervisor"),
        };
        Ok(PtyCommand {
            program: program.as_os_str().to_owned(),
            args: args.iter().map(Into::into).collect(),
        })
    }
}

/// Same Windows preference and standard install paths as the active app's
/// `src/main/terminal/profiles.ts`. Never reads production Shellfox preferences.
pub fn discover_shell_profiles() -> Result<Vec<ShellProfile>> {
    #[cfg(windows)]
    {
        use ShellProfileId::*;
        let system = crate::windows::system_directory()?;
        let pwsh = std::env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .map(|p| p.join("PowerShell/7/pwsh.exe"))
            .filter(|p| p.is_file())
            .or_else(|| {
                crate::environment::installed_path()
                    .into_iter()
                    .map(|p| p.join("pwsh.exe"))
                    .find(|p| p.is_file())
            });
        Ok(vec![
            local(Pwsh, "PowerShell", pwsh),
            local(WindowsPowerShell, "Windows PowerShell", Some(system.join("WindowsPowerShell/v1.0/powershell.exe"))),
            local(Cmd, "Command Prompt", Some(system.join("cmd.exe"))),
            ShellProfile { id: Wsl, label: "WSL", executable: None,
                unavailable_reason: Some("Guest process ownership is not implemented; a Windows Job cannot safely own WSL descendants.") },
            ShellProfile { id: LoginShell, label: "Login shell", executable: None,
                unavailable_reason: Some("Unix login shells require a platform process-tree supervisor.") },
        ])
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use ShellProfileId::*;
        let installed = [(Bash, "Bash"), (Sh, "sh"), (Zsh, "Zsh"), (Fish, "Fish")]
            .into_iter()
            .filter_map(|(id, label)| {
                let dirs = if cfg!(target_os = "macos") {
                    &["/bin", "/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"][..]
                } else {
                    &["/bin", "/usr/bin", "/usr/local/bin"][..]
                };
                let executable = dirs.iter()
                    .map(|dir| Path::new(dir).join(id.as_str()))
                    .find(|path| unix_executable(path))?;
                Some(ShellProfile {
                    id,
                    label,
                    executable: Some(executable),
                    unavailable_reason: None,
                })
            })
            .collect::<Vec<_>>();
        // Keep the persisted default ID, even when its executable is also listed by name.
        let executable = installed.iter()
            .find(|profile| cfg!(target_os = "macos") && profile.id == Zsh)
            .or_else(|| installed.first())
            .and_then(|profile| profile.executable.clone());
        let default = ShellProfile {
            id: LoginShell,
            label: "Shell",
            unavailable_reason: executable
                .is_none()
                .then_some("No supported Unix shell found."),
            executable,
        };
        Ok(std::iter::once(default).chain(installed).collect())
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        Ok([(ShellProfileId::LoginShell, "Login shell"), (ShellProfileId::Pwsh, "PowerShell"),
            (ShellProfileId::WindowsPowerShell, "Windows PowerShell"), (ShellProfileId::Cmd, "Command Prompt"),
            (ShellProfileId::Wsl, "WSL")].into_iter().map(|(id, label)| ShellProfile { id, label, executable: None,
                unavailable_reason: Some("Local shell launch requires a platform process-tree supervisor; no unsafe fallback is allowed.") }).collect())
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn unix_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|info| info.is_file() && info.permissions().mode() & 0o111 != 0)
}

#[cfg(windows)]
fn local(id: ShellProfileId, label: &'static str, path: Option<PathBuf>) -> ShellProfile {
    let executable = path.filter(|p| p.is_absolute() && p.is_file());
    let unavailable_reason = executable
        .is_none()
        .then_some("Executable was not found at a validated installation path.");
    ShellProfile {
        id,
        label,
        executable,
        unavailable_reason,
    }
}

pub fn default_shell_profile() -> Result<ShellProfile> {
    discover_shell_profiles()?
        .into_iter()
        .find(|p| p.unavailable_reason.is_none())
        .context("no safely supported local shell is available")
}

pub fn shell_profile(id: ShellProfileId) -> Result<ShellProfile> {
    let profile = discover_shell_profiles()?
        .into_iter()
        .find(|p| p.id == id)
        .with_context(|| {
            format!(
                "shell profile {} is not supported on this platform",
                id.as_str()
            )
        })?;
    profile.command()?;
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identifiers_are_strict_and_unavailable_profiles_cannot_launch() {
        for id in [
            ShellProfileId::Pwsh,
            ShellProfileId::WindowsPowerShell,
            ShellProfileId::Cmd,
            ShellProfileId::Wsl,
            ShellProfileId::LoginShell,
            ShellProfileId::Bash,
            ShellProfileId::Sh,
            ShellProfileId::Zsh,
            ShellProfileId::Fish,
        ] {
            assert_eq!(id.as_str().parse::<ShellProfileId>().unwrap(), id);
        }
        assert!("pwsh -Command whoami".parse::<ShellProfileId>().is_err());
        assert!(shell_profile(ShellProfileId::Wsl).is_err());
        for profile in discover_shell_profiles().unwrap() {
            if profile.unavailable_reason().is_some() {
                assert!(profile.command().is_err());
            }
        }
    }
}
