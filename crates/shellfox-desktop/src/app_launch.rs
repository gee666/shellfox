//! External launch adapter. Existing Options/main/Root compose these helpers;
//! no helper starts a process, edits integration settings or constructs a Host.
mod coordinator;
pub mod path;
#[cfg(test)]
mod tests;
pub use coordinator::*;
use shellfox_core::SessionId;
pub use shellfox_service::desktop_launch::{
    DesktopLaunchIntent, LaunchAction, LaunchDirectoryPolicy, LaunchDisposition, LaunchError,
};
use std::collections::HashSet;

/// Input contains only launch/product arguments, after Options/CLI globals have
/// been separated. Resolve against the invoking process cwd before forwarding.
pub fn parse_launch_args(
    args: &[String],
    invoking_cwd: &str,
    directories: &impl LaunchDirectoryPolicy,
) -> Result<DesktopLaunchIntent, LaunchError> {
    let mut index = 0;
    let mut seen = HashSet::new();
    let mut new_session = false;
    let mut start = false;
    let mut cwd = None;
    let mut request_id = None;
    let mut disposition = LaunchDisposition::ExistingWindow;
    while let Some(flag) = args.get(index) {
        index += 1;
        if !seen.insert(flag.as_str()) {
            return Err(LaunchError::Invalid);
        }
        match flag.as_str() {
            "start" | "add" | "new" | "create" => {
                if new_session || cwd.is_some() || start {
                    return Err(LaunchError::Invalid);
                }
                new_session = true;
                start = true;
                let target = args.get(index).filter(|value| !value.starts_with("--"));
                let value = target.map(String::as_str).unwrap_or(".");
                if target.is_some() {
                    index += 1;
                }
                cwd = Some(path::resolve_launch_path(invoking_cwd, value)?);
            }
            "--new-session" => {
                if start {
                    return Err(LaunchError::Invalid);
                }
                new_session = true;
            }
            "--cwd" => {
                if start {
                    return Err(LaunchError::Invalid);
                }
                let value = args
                    .get(index)
                    .filter(|value| !value.starts_with("--"))
                    .ok_or(LaunchError::Invalid)?;
                index += 1;
                cwd = Some(path::normalize_launch_path(value)?);
            }
            "--new-window" => disposition = LaunchDisposition::NewWindow,
            "--launch-request-id" => {
                let value = args.get(index).ok_or(LaunchError::Invalid)?;
                index += 1;
                request_id = Some(
                    value
                        .parse::<SessionId>()
                        .map_err(|_| LaunchError::Invalid)?,
                );
            }
            _ => return Err(LaunchError::Invalid),
        }
    }
    if new_session != cwd.is_some() {
        return Err(LaunchError::Invalid);
    }
    let action = if let Some(cwd) = cwd {
        let cwd = directories.validate_directory(&cwd)?;
        shellfox_core::validation::cwd(&cwd).map_err(|_| LaunchError::Invalid)?;
        LaunchAction::NewSession { cwd }
    } else {
        LaunchAction::Show {}
    };
    let intent = DesktopLaunchIntent {
        request_id: request_id.unwrap_or_default(),
        action,
        disposition,
    };
    intent.validate()?;
    Ok(intent)
}
