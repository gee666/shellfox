//! Windows WSL shares: `\\wsl.localhost\<distro>\...` and the legacy `\\wsl$\...`.
//! Either slash is accepted, so a typed `//wsl.localhost/Debian/home` parses too.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WslShare<'a> {
    pub server: &'a str,
    pub distro: &'a str,
    /// The path inside the distro, without a leading separator.
    pub rest: &'a str,
}

impl WslShare<'_> {
    /// The Linux path the share names, as 0.9 mapped it.
    pub fn guest_path(&self) -> String {
        format!("/{}", self.rest.replace('\\', "/"))
    }
    /// The canonical spelling: backslashes, no empty or `.` components.
    pub fn windows_path(&self) -> String {
        let mut path = format!(r"\\{}\{}", self.server, self.distro);
        for part in self.rest.split(['/', '\\']) {
            if !matches!(part, "" | ".") {
                path.push('\\');
                path.push_str(part);
            }
        }
        path
    }
}

/// Parses a WSL share path. Other UNC servers, `\\?\` device forms and an empty
/// distro are not shares.
pub fn wsl_share(path: &str) -> Option<WslShare<'_>> {
    let separator = |c: char| c == '/' || c == '\\';
    let mut chars = path.chars();
    if !chars.next().is_some_and(separator) || !chars.next().is_some_and(separator) {
        return None;
    }
    let tail = &path[2..];
    let (server, after) = tail.split_once(separator)?;
    if !server.eq_ignore_ascii_case("wsl.localhost") && !server.eq_ignore_ascii_case("wsl$") {
        return None;
    }
    let (distro, rest) = after.split_once(separator).unwrap_or((after, ""));
    let valid = !distro.is_empty()
        && !matches!(distro, "." | "..")
        && !distro.contains([':', '?', '*'])
        && !distro.chars().any(char::is_control);
    valid.then_some(WslShare {
        server,
        distro,
        rest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_servers_and_slash_styles_name_the_distro_and_guest_path() {
        for (input, distro, guest, windows) in [
            (
                r"\\wsl.localhost\Debian\home\u\proj",
                "Debian",
                "/home/u/proj",
                r"\\wsl.localhost\Debian\home\u\proj",
            ),
            (r"\\WSL$\Ubuntu-24.04", "Ubuntu-24.04", "/", r"\\WSL$\Ubuntu-24.04"),
            (
                "//wsl.localhost/Debian/home/./u/",
                "Debian",
                "/home/./u/",
                r"\\wsl.localhost\Debian\home\u",
            ),
            (r"\\wsl$/Debian\tmp", "Debian", "/tmp", r"\\wsl$\Debian\tmp"),
        ] {
            let share = wsl_share(input).expect(input);
            assert_eq!(share.distro, distro, "{input}");
            assert_eq!(share.guest_path(), guest, "{input}");
            assert_eq!(share.windows_path(), windows, "{input}");
        }
    }

    #[test]
    fn other_paths_are_not_wsl_shares() {
        for input in [
            r"C:\work",
            "/home/u",
            r"\\server\share\x",
            r"\\wsl.localhost\",
            r"\\wsl.localhost",
            r"\\?\UNC\wsl$\Debian",
            r"\wsl$\Debian",
            r"\\wsl$\..\x",
        ] {
            assert_eq!(wsl_share(input), None, "{input}");
        }
    }
}
