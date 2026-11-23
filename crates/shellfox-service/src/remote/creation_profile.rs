use super::*;
use shellfox_core::ProfileId;
use wire::{ProfileEnvironment, TerminalProfile};

pub(super) fn for_windows_folder(
    state: &RemoteState,
    configured: &ProfileId,
    cwd: &str,
) -> Result<ProfileId, AppError> {
    let profiles = state.profiles.as_ref().ok_or_else(|| capability_unavailable("trusted launch profiles"))?;
    let id = windows_profile(&profiles.profiles, configured.as_str(), cwd)
        .ok_or_else(|| capability_unavailable("available host-account terminal profile for this folder"))?;
    ProfileId::new(id).map_err(host_error)
}

fn windows_profile<'a>(profiles: &'a [TerminalProfile], configured: &str, cwd: &str) -> Option<&'a str> {
    let local = |p: &&TerminalProfile| p.elevation.is_none() && matches!(p.environment, ProfileEnvironment::Local);
    if let Some(distro) = wsl_distro(cwd) {
        return profiles.iter().find(|p| {
            p.elevation.is_none() && matches!(p.environment, ProfileEnvironment::Wsl) && p.available
                && p.distro.as_deref().is_some_and(|name| name.eq_ignore_ascii_case(distro))
        }).map(|p| p.id.as_str());
    }
    // An explicitly configured local profile must be available. A WSL default
    // is not a local profile and must not receive a Windows drive folder.
    if let Some(profile) = profiles.iter().filter(local).find(|p| p.id == configured) {
        return profile.available.then_some(profile.id.as_str());
    }
    ["pwsh", "windows-powershell"].into_iter().find_map(|id| {
        profiles.iter().filter(local).find(|p| p.id == id && p.available)
    }).or_else(|| profiles.iter().filter(local).find(|p| p.available)).map(|p| p.id.as_str())
}

fn wsl_distro(cwd: &str) -> Option<&str> {
    let (server, path) = cwd.strip_prefix(r"\\")?.split_once('\\')?;
    if !server.eq_ignore_ascii_case("wsl.localhost") && !server.eq_ignore_ascii_case("wsl$") { return None; }
    let distro = path.split('\\').next()?;
    (!distro.is_empty() && !distro.contains('/')).then_some(distro)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str, distro: Option<&str>, available: bool, elevated: bool) -> TerminalProfile {
        TerminalProfile {
            id: id.into(), label: id.into(),
            environment: if distro.is_some() { ProfileEnvironment::Wsl } else { ProfileEnvironment::Local },
            executable: r"C:\fixture\shell.exe".into(), args: vec![],
            distro: distro.map(str::to_owned), available, unavailable_reason: None,
            can_terminate_descendants: None,
            elevation: elevated.then_some(wire::Elevation::Administrator),
        }
    }

    #[test]
    fn remote_create_routes_unc_to_its_available_distro_and_drives_to_local_shells() {
        let profiles = [profile("wsl:Debian", Some("Debian"), true, false),
            profile("wsl:Offline", Some("Offline"), false, false),
            profile("wsl:Admin", Some("Admin"), true, true),
            profile("pwsh", None, true, false), profile("cmd", None, false, false)];
        assert_eq!(windows_profile(&profiles, "pwsh", r"\\WSL.LOCALHOST\debian\work"), Some("wsl:Debian"));
        assert_eq!(windows_profile(&profiles, "pwsh", r"\\wsl$\Debian"), Some("wsl:Debian"));
        for cwd in [r"\\wsl$\Missing\work", r"\\wsl$\Offline\work", r"\\wsl$\Admin\work"] {
            assert_eq!(windows_profile(&profiles, "pwsh", cwd), None);
        }
        assert_eq!(windows_profile(&profiles, "wsl:Debian", r"C:\fixture"), Some("pwsh"));
        assert_eq!(windows_profile(&profiles, "cmd", r"C:\fixture"), None);
    }
}
