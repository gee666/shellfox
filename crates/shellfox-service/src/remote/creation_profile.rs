use super::*;
use shellfox_core::ProfileId;

/// [`crate::folder_profile`] on Windows over the trusted launch catalog.
pub(super) fn for_windows_folder(
    state: &RemoteState,
    configured: &ProfileId,
    cwd: &str,
) -> Result<ProfileId, AppError> {
    let profiles = state
        .profiles
        .as_ref()
        .ok_or_else(|| capability_unavailable("trusted launch profiles"))?;
    let id = windows_profile(&profiles.profiles, configured.as_str(), cwd).ok_or_else(|| {
        capability_unavailable("available host-account terminal profile for this folder")
    })?;
    ProfileId::new(id).map_err(host_error)
}

fn windows_profile<'a>(
    profiles: &'a [wire::TerminalProfile],
    configured: &str,
    cwd: &str,
) -> Option<&'a str> {
    let candidates: Vec<crate::LaunchCandidate<'a>> = profiles.iter().map(Into::into).collect();
    crate::folder_profile(&candidates, configured, cwd, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wire::{ProfileEnvironment, TerminalProfile};

    fn profile(id: &str, distro: Option<&str>, available: bool, elevated: bool) -> TerminalProfile {
        TerminalProfile {
            id: id.into(),
            label: id.into(),
            environment: if distro.is_some() {
                ProfileEnvironment::Wsl
            } else {
                ProfileEnvironment::Local
            },
            executable: r"C:\fixture\shell.exe".into(),
            args: vec![],
            distro: distro.map(str::to_owned),
            available,
            unavailable_reason: None,
            can_terminate_descendants: None,
            elevation: elevated.then_some(wire::Elevation::Administrator),
        }
    }

    #[test]
    fn remote_create_routes_unc_to_its_available_distro_and_drives_to_local_shells() {
        let profiles = [
            profile("wsl:Debian", Some("Debian"), true, false),
            profile("wsl:Offline", Some("Offline"), false, false),
            profile("wsl:Admin", Some("Admin"), true, true),
            profile("pwsh", None, true, false),
            profile("cmd", None, false, false),
        ];
        assert_eq!(
            windows_profile(&profiles, "pwsh", r"\\WSL.LOCALHOST\debian\work"),
            Some("wsl:Debian")
        );
        assert_eq!(
            windows_profile(&profiles, "pwsh", r"\\wsl$\Debian"),
            Some("wsl:Debian")
        );
        for cwd in [
            r"\\wsl$\Missing\work",
            r"\\wsl$\Offline\work",
            r"\\wsl$\Admin\work",
        ] {
            assert_eq!(windows_profile(&profiles, "pwsh", cwd), None);
        }
        assert_eq!(
            windows_profile(&profiles, "wsl:Debian", r"C:\fixture"),
            Some("pwsh")
        );
        assert_eq!(windows_profile(&profiles, "cmd", r"C:\fixture"), None);
    }
}
