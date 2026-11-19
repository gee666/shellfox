//! A guest path is meaningful only with a WSL distribution from a session or tab.
use crate::{ErrorCode, FileError, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WslBrowserScope {
    distro: String,
    cwd: String,
}

impl WslBrowserScope {
    /// Only the host-recorded WSL profile supplies distro authority, never a path or OSC report.
    pub fn for_tab(cwd: &str, profile: &str) -> Result<Self> {
        let distro = profile.strip_prefix("wsl:").filter(|name| valid_distro(name))
            .ok_or_else(FileError::denied)?;
        let guest = if let Some((name, guest)) = parse_unc(cwd) {
            if !name.eq_ignore_ascii_case(distro) {
                return Err(FileError::denied());
            }
            guest
        } else if cwd.starts_with('/') && !cwd.starts_with("//") && !cwd.contains('\\') {
            cwd.to_owned()
        } else if cwd.as_bytes().get(1) == Some(&b':') {
            // A drive cwd can seed a WSL launch, but it is not a guest path.
            "/".to_owned()
        } else {
            return Err(FileError::denied());
        };
        let scope = Self { distro: distro.into(), cwd: "/".into() };
        Ok(Self { cwd: scope.guest(&guest)?, ..scope })
    }

    pub fn distro(&self) -> &str {
        &self.distro
    }
    pub fn guest_cwd(&self) -> &str {
        &self.cwd
    }
    /// Translate through this distribution only; unrelated UNC shares are never opened.
    pub fn guest(&self, input: &str) -> Result<String> {
        self.guest_from(input, None)
    }

    pub fn guest_from(&self, input: &str, working_directory: Option<&str>) -> Result<String> {
        if input.len() > 32767 || input.chars().any(char::is_control) {
            return Err(FileError::new(
                ErrorCode::Validation,
                "Invalid guest file path.",
            ));
        }
        if input == "~" || input.starts_with("~/") {
            return Err(FileError::new(
                ErrorCode::Unsupported,
                "Use an absolute WSL path rather than ~.",
            ));
        }
        if let Some((distro, guest)) = parse_unc(input) {
            if !distro.eq_ignore_ascii_case(&self.distro) {
                return Err(FileError::new(
                    ErrorCode::AccessDenied,
                    "The path belongs to another WSL distribution.",
                ));
            }
            return Ok(normalize_guest("/", &guest));
        }
        let drive =
            input.as_bytes().get(1) == Some(&b':') && input.as_bytes()[0].is_ascii_alphabetic();
        if drive || input.contains('\\') || input.starts_with("//") {
            return Err(FileError::new(
                ErrorCode::Validation,
                "Expected a path in this WSL distribution.",
            ));
        }
        let live_cwd = match working_directory {
            Some(cwd) if !input.starts_with('/') => Some(self.guest(cwd)?),
            _ => None,
        };
        Ok(normalize_guest(live_cwd.as_deref().unwrap_or(&self.cwd), input))
    }
}

fn parse_unc(path: &str) -> Option<(&str, String)> {
    let (server, rest) = path.strip_prefix("\\\\")?.split_once('\\')?;
    if !server.eq_ignore_ascii_case("wsl.localhost") && !server.eq_ignore_ascii_case("wsl$") {
        return None;
    }
    let (distro, path) = rest.split_once('\\').unwrap_or((rest, ""));
    valid_distro(distro).then(|| (distro, format!("/{}", path.replace('\\', "/"))))
}

fn valid_distro(value: &str) -> bool {
    !value.is_empty()
        && value.encode_utf16().count() <= 160
        && value.trim() == value
        && !value.starts_with('-')
        && !matches!(value, "." | "..")
        && !value.contains(['/', '\\', ':', '?', '*'])
        && !value.chars().any(char::is_control)
}

fn normalize_guest(base: &str, requested: &str) -> String {
    let prefix = if requested.starts_with('/') { "" } else { base };
    let mut parts = Vec::new();
    for part in prefix.split('/').chain(requested.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            component => parts.push(component),
        }
    }
    format!("/{}", parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_and_current_wsl_share_names_stay_in_the_session_distribution() {
        let scope = WslBrowserScope::for_tab(r"\\wsl$\Debian\home\user", "wsl:Debian").unwrap();
        assert_eq!(scope.guest_cwd(), "/home/user");
        assert_eq!(scope.guest("../note.txt").unwrap(), "/home/note.txt");
        assert_eq!(
            scope.guest(r"\\WSL.LOCALHOST\DEBIAN\home\user").unwrap(),
            "/home/user"
        );
        assert_eq!(
            scope.guest(r"\\wsl$\debian\etc\os-release").unwrap(),
            "/etc/os-release"
        );
        assert!(scope.guest(r"\\wsl$\Ubuntu\etc").is_err());
        assert!(scope.guest(r"\\server\share\etc").is_err());
        assert!(scope.guest(r"C:\Windows").is_err());
        assert!(scope.guest("~/file").is_err());
        assert_eq!(scope.guest("../../../../../root").unwrap(), "/root");
    }

    #[test]
    fn only_the_tab_profile_can_bind_the_distro() {
        let scope = WslBrowserScope::for_tab(r"\\wsl$\debian\tmp", "wsl:Debian").unwrap();
        assert_eq!(scope.distro(), "Debian");
        assert_eq!(scope.guest_cwd(), "/tmp");
        assert!(WslBrowserScope::for_tab(r"\\wsl$\Other\tmp", "wsl:Debian").is_err());
        assert!(WslBrowserScope::for_tab(r"\\wsl$\Debian\tmp", "pwsh").is_err());
        assert!(WslBrowserScope::for_tab(r"\\server\share\tmp", "wsl:Debian").is_err());
        assert!(scope.guest("//server/share").is_err());
    }

    #[test]
    fn terminal_links_resolve_in_their_tab_not_the_visible_files_panel() {
        let scope = WslBrowserScope::for_tab("/initial", "wsl:Debian").unwrap();
        assert_eq!(scope.guest_from("./note.txt", None).unwrap(), "/initial/note.txt");
        assert_eq!(
            scope.guest_from("../note.txt", Some(r"\\wsl$\Debian\dir\sub")).unwrap(),
            "/dir/note.txt"
        );
        assert_eq!(
            scope.guest_from("/tmp/note.txt", Some("/a/different/cwd")).unwrap(),
            "/tmp/note.txt"
        );
        assert!(scope.guest_from("C:/host-only.txt", Some("/tmp")).is_err());
        assert!(scope.guest_from("./note.txt", Some(r"\\wsl$\Other\tmp")).is_err());
        assert!(scope.guest_from(r"\\server\share\note.txt", Some("/tmp")).is_err());
    }
}
