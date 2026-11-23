use shellfox_service::desktop_launch::LaunchError;

/// Lexical resolution only. The invoking and owning system adapters must also
/// validate directory existence/access and the local-drive/WSL policy.
pub fn resolve_launch_path(base: &str, target: &str) -> Result<String, LaunchError> {
    shellfox_core::validation::cwd(base).map_err(|_| LaunchError::Invalid)?;
    let target = if target.is_empty() { "." } else { target };
    if target.chars().any(char::is_control) || target.contains("://") {
        return Err(LaunchError::Invalid);
    }
    let windows = drive(base) || base.starts_with("\\\\");
    let path = if windows {
        let target = target.replace('/', "\\");
        if drive(&target) || target.starts_with("\\\\") {
            target
        } else if target.starts_with('\\') {
            let base = base.replace('/', "\\");
            let (root, _) = windows_root(&base)?;
            format!("{}{target}", root.trim_end_matches('\\'))
        } else if target.as_bytes().get(1) == Some(&b':') {
            if !drive(base) || !base.as_bytes()[0].eq_ignore_ascii_case(&target.as_bytes()[0]) {
                // Other-drive relative paths require ambient per-drive cwd.
                return Err(LaunchError::Invalid);
            }
            format!(
                "{}{}\\{}",
                &target[..2],
                base[2..].trim_end_matches(['\\', '/']),
                &target[2..]
            )
        } else if target.contains(':') {
            return Err(LaunchError::Invalid);
        } else {
            format!("{}\\{target}", base.trim_end_matches(['\\', '/']))
        }
    } else if target.starts_with('/') {
        target.to_owned()
    } else {
        format!("{}/{target}", base.trim_end_matches('/'))
    };
    normalize_absolute(&path, windows)
}
pub fn normalize_launch_path(path: &str) -> Result<String, LaunchError> {
    normalize_absolute(path, drive(path) || path.starts_with("\\\\"))
}
fn drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}
fn windows_root(path: &str) -> Result<(String, &str), LaunchError> {
    if drive(path) {
        if path[2..].contains(':') {
            return Err(LaunchError::Invalid);
        }
        return Ok((path[..3].to_owned(), &path[3..]));
    }
    let server_end = path[2..].find('\\').ok_or(LaunchError::Invalid)? + 2;
    let distro_start = server_end + 1;
    let distro_end = path[distro_start..]
        .find('\\')
        .map(|index| index + distro_start)
        .unwrap_or(path.len());
    if distro_end == distro_start || path.contains(':') {
        return Err(LaunchError::Invalid);
    }
    Ok((
        format!("{}\\", &path[..distro_end]),
        path.get(distro_end + 1..).unwrap_or(""),
    ))
}
fn normalize_absolute(path: &str, windows: bool) -> Result<String, LaunchError> {
    shellfox_core::validation::cwd(path).map_err(|_| LaunchError::Invalid)?;
    let path = if windows {
        path.replace('/', "\\")
    } else {
        path.to_owned()
    };
    let (root, rest) = if windows {
        windows_root(&path)?
    } else {
        ("/".to_owned(), &path[1..])
    };
    let separator = if windows { '\\' } else { '/' };
    let mut parts = Vec::new();
    for part in rest.split(separator) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    let normalized = format!("{root}{}", parts.join(&separator.to_string()));
    shellfox_core::validation::cwd(&normalized).map_err(|_| LaunchError::Invalid)?;
    Ok(normalized)
}
