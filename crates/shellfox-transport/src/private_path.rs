use crate::HelperError;
use std::path::{Component, Path, PathBuf};

/// Convert only ordinary Windows drive/UNC forms and their canonical verbatim
/// equivalents. Never resolve links or reinterpret device namespaces.
pub fn normalize_private_path(path: &Path) -> Result<PathBuf, HelperError> {
    #[cfg(windows)]
    let text = path.to_str().ok_or(HelperError::InvalidRequest)?;
    #[cfg(not(windows))]
    let text = path.to_string_lossy(); // Validate without changing the path's native bytes.
    if !path.is_absolute() || path.as_os_str().len() > 32767 || text.chars().any(char::is_control) {
        return Err(HelperError::InvalidRequest);
    }
    #[cfg(windows)]
    {
        windows_path(path, text)
    }
    #[cfg(not(windows))]
    {
        if path
            .components()
            .any(|c| matches!(c, Component::CurDir | Component::ParentDir))
            || text.split('/').any(|c| c == "." || c == "..")
        {
            return Err(HelperError::InvalidRequest);
        }
        Ok(path.to_path_buf())
    }
}

#[cfg(windows)]
fn windows_path(path: &Path, text: &str) -> Result<PathBuf, HelperError> {
    use std::path::Prefix;
    // Components can discard dot segments. Reject them before conversion.
    if text.split(['\\', '/']).any(|c| c == "." || c == "..") {
        return Err(HelperError::InvalidRequest);
    }
    let mut parts = path.components();
    let Some(Component::Prefix(prefix)) = parts.next() else {
        return Err(HelperError::InvalidRequest);
    };
    let mut output = match prefix.kind() {
        Prefix::Disk(disk) | Prefix::VerbatimDisk(disk) if disk.is_ascii_alphabetic() => {
            PathBuf::from(format!("{}:\\", char::from(disk)))
        }
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
            let server = server.to_str().ok_or(HelperError::InvalidRequest)?;
            let share = share.to_str().ok_or(HelperError::InvalidRequest)?;
            valid_component(server)?;
            valid_component(share)?;
            if server.eq_ignore_ascii_case("GLOBALROOT") {
                return Err(HelperError::InvalidRequest);
            }
            PathBuf::from(format!("\\\\{server}\\{share}\\"))
        }
        _ => return Err(HelperError::InvalidRequest),
    };
    if !matches!(parts.next(), Some(Component::RootDir)) {
        return Err(HelperError::InvalidRequest);
    }
    for part in parts {
        let Component::Normal(part) = part else {
            return Err(HelperError::InvalidRequest);
        };
        valid_component(part.to_str().ok_or(HelperError::InvalidRequest)?)?;
        output.push(part);
    }
    Ok(output)
}
#[cfg(windows)]
fn valid_component(value: &str) -> Result<(), HelperError> {
    if value.is_empty()
        || value.ends_with(['.', ' '])
        || value
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        return Err(HelperError::InvalidRequest);
    }
    let stem = value
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    let numbered_device = ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|suffix| {
            ["1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"].contains(&suffix)
        })
    });
    if ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "CLOCK$"].contains(&stem.as_str())
        || numbered_device
    {
        return Err(HelperError::InvalidRequest);
    }
    Ok(())
}

#[cfg(all(test, unix))]
#[test]
fn unix_private_paths_preserve_native_bytes_and_reject_ambiguous_components() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let path = Path::new(OsStr::from_bytes(b"/tmp/private-\xff"));
    assert_eq!(normalize_private_path(path).unwrap(), path);
    for invalid in [
        b"/tmp/./private-\xff".as_slice(),
        b"/tmp/private-\xff/..",
        b"/tmp/\x00\xff",
    ] {
        assert_eq!(
            normalize_private_path(Path::new(OsStr::from_bytes(invalid))),
            Err(HelperError::InvalidRequest)
        );
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn canonical_disk_and_unc_normalize_without_namespace_aliases() {
        for (input, expected) in [
            (r"\\?\P:\work\private.json", r"P:\work\private.json"),
            (r"\\?\UNC\server\share\private", r"\\server\share\private"),
            ("P:/work/private.json", r"P:\work\private.json"),
        ] {
            assert_eq!(
                normalize_private_path(Path::new(input)).unwrap(),
                PathBuf::from(expected)
            );
        }
        for input in [
            r"\\.\PhysicalDrive0",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\file",
            r"\\?\P:\work\file.",
            r"\\?\P:\work\file ",
            r"\\?\P:\work\file:stream",
            r"\\?\P:\work\CON.txt",
            r"\\?\P:\work\COM¹",
            r"\\?\P:\work\..\file",
            r"\\?\P:\work\.\file",
            r"P:relative",
        ] {
            assert_eq!(
                normalize_private_path(Path::new(input)),
                Err(HelperError::InvalidRequest),
                "{input}"
            );
        }
    }
}
