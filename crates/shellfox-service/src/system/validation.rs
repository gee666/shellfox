use super::{error, SystemResult};
use shellfox_protocol::ErrorCode;
use std::path::{Component, Path};

pub fn absolute_path(path: &Path) -> SystemResult<()> {
    #[cfg(windows)]
    let value = path
        .to_str()
        .ok_or_else(|| error(ErrorCode::Validation, "Path encoding is unsupported."))?;
    #[cfg(not(windows))]
    let value = path.to_string_lossy(); // Inspect delimiters and controls, not the native path returned to callers.
    if !path.is_absolute()
        || path.as_os_str().len() > 32767
        || value.chars().any(char::is_control)
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        || value
            .split(['/', '\\'])
            .any(|part| part == "." || part == "..")
    {
        return Err(error(
            ErrorCode::Validation,
            "Expected a bounded absolute path without dot components or controls.",
        ));
    }
    #[cfg(windows)]
    {
        use std::path::Prefix;
        if !matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::UNC(_, _)))
        {
            return Err(error(
                ErrorCode::Validation,
                "Device namespace paths are not accepted from system requests.",
            ));
        }
        shellfox_transport::normalize_private_path(path)
            .map_err(|_| error(ErrorCode::Validation, "Ambiguous Windows path refused."))?;
    }
    Ok(())
}

/// URI syntax is not launch authorization. Only perform_user_action reaches an
/// opener; discovery and projection never invoke a browser or shell.
pub fn web_url(value: &str) -> SystemResult<String> {
    if value.is_empty()
        || value.len() > 2048
        || value.chars().any(char::is_control)
        || value.contains('\\')
    {
        return Err(error(
            ErrorCode::Validation,
            "Web links must be bounded http(s) URLs without controls.",
        ));
    }
    let uri =
        url::Url::parse(value).map_err(|_| error(ErrorCode::Validation, "Invalid web URL."))?;
    if !matches!(uri.scheme(), "http" | "https") || uri.host_str().is_none_or(str::is_empty) {
        return Err(error(
            ErrorCode::Validation,
            "Only absolute http(s) links are allowed.",
        ));
    }
    Ok(uri.into())
}
