//! Settings normalization over facts supplied by trusted host discovery.
//! This module neither discovers nor launches executables.
use crate::*;
#[derive(Clone, Debug)]
pub struct SettingsProfile {
    pub id: String,
    pub shell_id: SettingsShell,
    pub executable: String,
    pub available: bool,
    pub elevation: bool,
}
/// Windows paths match whatever their separators or case: earlier builds
/// saved "PowerShell/7/pwsh.exe", 0.9 and discovery "PowerShell\7\pwsh.exe".
fn same_executable(saved: &str, discovered: &str) -> bool {
    if cfg!(windows) {
        let normal = |path: &str| path.replace('/', "\\").to_lowercase();
        normal(saved) == normal(discovered)
    } else {
        saved == discovered
    }
}
/// Mirrors src/main/terminal/service.ts::saveSettings. The adapter must validate
/// Linux Python separately and provide CURRENT discovery, not client-supplied facts.
pub fn normalize_settings_profile(
    previous: &Settings,
    mut candidate: Settings,
    profiles: &[SettingsProfile],
    default_profile_id: Option<&str>,
) -> Result<Settings> {
    let explicit = candidate
        .terminal_profile_id
        .as_ref()
        .and_then(Option::as_deref);
    let shell_id = match candidate.shell_id {
        SettingsShell::Pwsh => "pwsh",
        SettingsShell::WindowsPowershell => "windows-powershell",
        SettingsShell::Bash => "bash",
        SettingsShell::LoginShell => "login-shell",
        SettingsShell::Wsl => "wsl",
    };
    let requested = explicit
        .or_else(|| {
            profiles
                .iter()
                .find(|p| {
                    p.id == shell_id
                        && candidate
                            .shell_executable
                            .as_ref()
                            .is_none_or(|exe| same_executable(exe, &p.executable))
                })
                .map(|p| p.id.as_str())
        })
        .or(default_profile_id);
    let selected = profiles
        .iter()
        .find(|p| Some(p.id.as_str()) == requested && p.available && !p.elevation)
        .ok_or_else(|| {
            Error::new(
                ErrorCode::Validation,
                "Choose a discovered terminal profile.",
            )
        })?;
    let unchanged_executable =
        explicit.is_some() && candidate.shell_executable == previous.shell_executable;
    validation::ensure(
        candidate
            .shell_executable
            .as_ref()
            .is_none_or(|exe| same_executable(exe, &selected.executable))
            || unchanged_executable,
        "Arbitrary shell executables are not allowed.",
    )?;
    candidate.adapter_id = SettingsAdapter::EmbeddedPty;
    candidate.terminal_profile_id = Some(Some(selected.id.clone()));
    candidate.shell_id = selected.shell_id;
    candidate.shell_executable = Some(selected.executable.clone());
    candidate.validate()?;
    Ok(candidate)
}

#[cfg(test)]
mod executable_tests {
    #[test]
    fn saved_windows_paths_match_across_separators() {
        let discovered = r"C:\Program Files\PowerShell\7\pwsh.exe";
        assert_eq!(
            super::same_executable(r"C:\Program Files\PowerShell/7/pwsh.exe", discovered),
            cfg!(windows)
        );
        assert!(super::same_executable(discovered, discovered));
        assert!(!super::same_executable(r"C:\other\pwsh.exe", discovered));
    }
}
