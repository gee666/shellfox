//! The profile a new terminal gets for its folder when none was asked for. Every
//! route uses this: createSession, addTab, desktop, mobile, CLI and explorer.

/// A catalog entry as the choice needs it.
#[derive(Clone, Copy, Debug)]
pub struct LaunchCandidate<'a> {
    pub id: &'a str,
    pub available: bool,
    pub elevated: bool,
}

impl<'a> From<&'a shellfox_protocol::TerminalProfile> for LaunchCandidate<'a> {
    fn from(profile: &'a shellfox_protocol::TerminalProfile) -> Self {
        Self {
            id: &profile.id,
            available: profile.available,
            elevated: profile.elevation.is_some(),
        }
    }
}

fn is_wsl(id: &str) -> bool {
    id == "wsl" || id.starts_with("wsl:")
}

/// As 0.9's TerminalService.profile: on Windows a WSL share opens in its own
/// distro, and any other folder in the saved local profile, else PowerShell.
/// A saved WSL default never receives a Windows folder. Elsewhere it is the
/// saved default when usable, else the first usable profile.
pub fn folder_profile<'a>(
    candidates: &[LaunchCandidate<'a>],
    configured: &str,
    cwd: &str,
    windows: bool,
) -> Option<&'a str> {
    let usable = |p: &&LaunchCandidate<'a>| p.available && !p.elevated;
    if !windows {
        return crate::effective_default_profile(
            configured,
            candidates.iter().filter(usable).map(|p| p.id),
        );
    }
    if let Some(share) = shellfox_core::wsl_share(cwd) {
        return candidates
            .iter()
            .filter(usable)
            .find(|p| {
                p.id.strip_prefix("wsl:")
                    .is_some_and(|distro| distro.eq_ignore_ascii_case(share.distro))
            })
            .map(|p| p.id);
    }
    let local = |p: &&LaunchCandidate<'a>| !p.elevated && !is_wsl(p.id);
    // A saved local profile that is unavailable is an error, not a fallback.
    if let Some(saved) = candidates.iter().filter(local).find(|p| p.id == configured) {
        return saved.available.then_some(saved.id);
    }
    ["pwsh", "windows-powershell"]
        .into_iter()
        .find_map(|id| candidates.iter().filter(usable).find(|p| p.id == id))
        .or_else(|| candidates.iter().filter(usable).find(local))
        .map(|p| p.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates() -> Vec<LaunchCandidate<'static>> {
        [
            ("pwsh", true, false),
            ("windows-powershell", true, false),
            ("windows-powershell-admin", true, true),
            ("cmd", false, false),
            ("wsl:Debian", true, false),
            ("wsl:Offline", false, false),
            ("login-shell", true, false),
        ]
        .map(|(id, available, elevated)| LaunchCandidate {
            id,
            available,
            elevated,
        })
        .into()
    }

    #[test]
    fn windows_wsl_shares_open_in_their_available_distro() {
        let all = candidates();
        for cwd in [
            r"\\wsl.localhost\Debian\home\u\proj",
            r"\\WSL$\debian",
            "//wsl.localhost/Debian/home",
        ] {
            for saved in ["pwsh", "wsl:Offline", "cmd"] {
                assert_eq!(
                    folder_profile(&all, saved, cwd, true),
                    Some("wsl:Debian"),
                    "{cwd} {saved}"
                );
            }
        }
        for cwd in [r"\\wsl$\Offline\x", r"\\wsl$\Missing\x"] {
            assert_eq!(folder_profile(&all, "pwsh", cwd, true), None, "{cwd}");
        }
    }

    #[test]
    fn windows_folders_use_the_saved_local_profile_never_a_wsl_default() {
        let mut all = candidates();
        assert_eq!(
            folder_profile(&all, "wsl:Debian", r"C:\work", true),
            Some("pwsh")
        );
        assert_eq!(
            folder_profile(&all, "windows-powershell", r"C:\work", true),
            Some("windows-powershell")
        );
        assert_eq!(
            folder_profile(&all, "windows-powershell-admin", r"C:\work", true),
            Some("pwsh")
        );
        assert_eq!(folder_profile(&all, "cmd", r"C:\work", true), None);
        all[0].available = false;
        assert_eq!(
            folder_profile(&all, "wsl:Debian", r"C:\work", true),
            Some("windows-powershell")
        );
    }

    #[test]
    fn other_hosts_use_the_effective_default() {
        let all = candidates();
        assert_eq!(
            folder_profile(&all, "login-shell", "/home/u", false),
            Some("login-shell")
        );
        assert_eq!(
            folder_profile(&all, "missing", r"\\wsl$\Debian\x", false),
            Some("pwsh")
        );
    }
}
