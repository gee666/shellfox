use std::path::Path;
use url::Url;

/// A terminal's OSC 7 can propose a path, never a remote host or filesystem
/// authority. Files still opens it through the current local backend.
pub(super) fn from_report(report: &str) -> Option<String> {
    let (scheme, rest) = report.split_at_checked(7)?;
    let (authority, path) = rest.split_once('/')?;
    // The URL parser can discard a host when a Windows drive follows it.
    if !scheme.eq_ignore_ascii_case("file://")
        || !(authority.is_empty() || authority.eq_ignore_ascii_case("localhost"))
        || network_style(path)
    {
        return None;
    }
    let mut url = Url::parse(report).ok()?;
    if url.scheme() != "file"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    match url.host_str() {
        None => {}
        Some(host) if host.eq_ignore_ascii_case("localhost") => url.set_host(None).ok()?,
        Some(_) => return None,
    }
    // `to_file_path` folds empty path segments on Unix. Reject network-style
    // paths before conversion can turn them into ordinary absolute paths.
    let relative = url.path().strip_prefix('/')?;
    if network_style(relative) {
        return None;
    }
    let path = url.to_file_path().ok()?;
    let text = path.to_str()?;
    (local_absolute(&path) && shellfox_core::validation::cwd(text).is_ok()).then(|| text.to_owned())
}

fn network_style(path_after_slash: &str) -> bool {
    path_after_slash.starts_with(['/', '\\'])
        || path_after_slash.get(..3).is_some_and(|prefix| {
            prefix.eq_ignore_ascii_case("%2f") || prefix.eq_ignore_ascii_case("%5c")
        })
}

#[cfg(windows)]
fn local_absolute(path: &Path) -> bool {
    use std::path::{Component, Prefix};
    matches!(path.components().next(), Some(Component::Prefix(prefix))
        if matches!(prefix.kind(), Prefix::Disk(_)))
        && path.is_absolute()
        && !path
            .to_string_lossy()
            .get(2..)
            .is_some_and(|tail| tail.contains(':'))
}

#[cfg(not(windows))]
fn local_absolute(path: &Path) -> bool {
    path.is_absolute()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_only_absolute_local_file_reports() {
        let (report, expected) = if cfg!(windows) {
            ("file:///C:/work/next%20folder", r"C:\work\next folder")
        } else {
            ("file:///work/next%20folder", "/work/next folder")
        };
        assert_eq!(from_report(report).as_deref(), Some(expected));
        let localhost = report.replacen("file://", "file://localhost", 1);
        assert_eq!(from_report(&localhost).as_deref(), Some(expected));
        for value in [
            "",
            "file:relative",
            "https://localhost/work",
            "file://other.invalid/work",
            "file://other.invalid/C:/work",
            "file:////other.invalid/share",
            "file://localhost//other.invalid/share",
            "file:///%2Fother.invalid/share",
            "file:///C:/work%00name",
            "file:///C:/work?query",
            "file:///C:/work#fragment",
        ] {
            assert_eq!(from_report(value), None, "{value}");
        }
        #[cfg(windows)]
        assert_eq!(from_report("file:///C:/work/target:alternate"), None);
    }
}
