use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct PathError {
    pub message: String,
    pub missing: bool,
}

impl PathError {
    fn validation(message: &str) -> Self {
        Self {
            message: message.into(),
            missing: false,
        }
    }

    fn io(error: io::Error, action: &str) -> Self {
        let kind = error.kind();
        let detail = match kind {
            io::ErrorKind::NotFound => "directory does not exist.",
            io::ErrorKind::NotADirectory => "path is not a directory.",
            io::ErrorKind::PermissionDenied => "directory is not accessible.",
            _ if action == "Cannot complete path" => {
                return Self::validation("Could not read directory completions.");
            }
            _ => "choose an accessible local or WSL directory.",
        };
        Self {
            message: format!("{action}: {detail}"),
            missing: kind == io::ErrorKind::NotFound,
        }
    }
}

fn valid_text(input: &str) -> bool {
    input.encode_utf16().count() <= 32767 && !input.bytes().any(|b| b.is_ascii_control())
}

pub(super) fn unquote(input: &str) -> &str {
    let trimmed = input.trim();
    match trimmed.as_bytes() {
        [first @ (b'\'' | b'"'), .., last] if first == last => &trimmed[1..trimmed.len() - 1],
        _ => input,
    }
}

fn path_text(input: &str) -> Result<&str, PathError> {
    if !valid_text(input) {
        return Err(PathError::validation(
            "Path must contain at most 32767 characters and no control characters.",
        ));
    }
    Ok(unquote(input))
}

pub(super) fn home() -> Option<PathBuf> {
    std::env::home_dir().filter(|path| path.is_absolute() && path.to_str().is_some())
}

fn is_separator(ch: char) -> bool {
    ch == '/' || cfg!(windows) && ch == '\\'
}

fn absolutize(home: &Path, value: &str) -> Result<PathBuf, PathError> {
    let drive = matches!(value.as_bytes(), [letter, b':', ..] if letter.is_ascii_alphabetic());
    let unc = value.chars().take(2).filter(|ch| is_separator(*ch)).count() == 2;
    if cfg!(windows) {
        if drive && !value[2..].starts_with(is_separator) {
            return Err(PathError::validation(
                "Use an absolute drive path, such as C:\\folder, not a drive-relative path.",
            ));
        }
        if unc {
            let mut parts = value[2..].split(is_separator);
            match (parts.next(), parts.next()) {
                (Some("." | "?"), Some(_)) => {
                    return Err(PathError::validation(
                        "Windows device paths are not supported.",
                    ));
                }
                (Some(server), Some(share)) if !server.is_empty() && !share.is_empty() => {}
                _ => {
                    return Err(PathError::validation(
                        "UNC paths must include a server and share.",
                    ));
                }
            }
        }
    } else if drive || unc || value.starts_with('\\') {
        return Err(PathError::validation(
            "Windows drive and UNC paths require Windows.",
        ));
    }
    let path = if value == "~" {
        home.to_owned()
    } else if value
        .strip_prefix('~')
        .is_some_and(|tail| tail.starts_with(is_separator))
    {
        // Concatenate before normalizing: a second separator does not make ~/ absolute.
        let mut path = home.as_os_str().to_owned();
        path.push(&value[1..]);
        PathBuf::from(path)
    } else {
        home.join(value)
    };
    normalize(path)
}

fn normalize(path: PathBuf) -> Result<PathBuf, PathError> {
    let normalized = path.components().fold(PathBuf::new(), |mut path, part| {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                path.pop();
            }
            _ => path.push(part.as_os_str()),
        }
        path
    });
    if normalized.is_absolute() {
        Ok(normalized)
    } else {
        Err(PathError::validation(
            "Home directory is not a supported absolute path.",
        ))
    }
}

fn completion_target<'a>(
    home: &Path,
    input: &'a str,
) -> Result<(PathBuf, String, &'a str, char), PathError> {
    let value = path_text(input)?;
    let absolute = absolutize(home, value)?;
    let split = value.rfind(is_separator);
    let separator = split.map_or(std::path::MAIN_SEPARATOR, |index| {
        char::from(value.as_bytes()[index])
    });
    if matches!(value, "~" | "." | "..")
        || Path::new(value).is_absolute() && Path::new(value).parent().is_none()
    {
        let prefix = if value.ends_with(separator) {
            value.to_owned()
        } else {
            format!("{value}{separator}")
        };
        return Ok((absolute, prefix, "", separator));
    }
    let (prefix, fragment) = value.split_at(split.map_or(0, |index| index + 1));
    Ok((
        absolutize(home, prefix)?,
        prefix.to_owned(),
        fragment,
        separator,
    ))
}

pub(super) fn complete(home: &Path, input: &str) -> Result<Vec<String>, PathError> {
    let (directory, prefix, fragment, separator) = completion_target(home, input)?;
    let key = |value: &str| {
        if cfg!(windows) {
            value.to_lowercase()
        } else {
            value.to_owned()
        }
    };
    let needle = key(fragment);
    let read_error = |error| PathError::io(error, "Cannot complete path");
    let mut entries = fs::read_dir(directory)
        .map_err(read_error)?
        .filter_map(|entry| match entry {
            Ok(entry) => entry
                .file_name()
                .into_string()
                .ok()
                .filter(|name| key(name).starts_with(&needle))
                .map(|name| Ok((name, entry))),
            Err(error) => Some(Err(read_error(error))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
    let mut matches = Vec::new();
    let mut bytes = 0;
    for (name, entry) in entries {
        let kind = entry.file_type().map_err(read_error)?;
        let directory = kind.is_dir()
            || kind.is_symlink()
                && match fs::metadata(entry.path()) {
                    Ok(metadata) => metadata.is_dir(),
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                        ) =>
                    {
                        false
                    }
                    Err(error) => return Err(read_error(error)),
                };
        if !directory {
            continue;
        }
        let candidate = format!("{prefix}{name}{separator}");
        if !valid_text(&candidate) {
            continue;
        }
        bytes += candidate.len();
        if bytes > 65536 || matches.len() == 100 {
            break;
        }
        matches.push(candidate);
    }
    Ok(matches)
}

pub(super) fn resolve(home: &Path, input: &str, create: bool) -> Result<String, PathError> {
    let action = if create {
        "Cannot create directory"
    } else {
        "Cannot resolve directory"
    };
    let path = absolutize(home, path_text(input)?)?;
    let cwd = path
        .to_str()
        .filter(|path| shellfox_core::validation::cwd(path).is_ok())
        .ok_or_else(|| PathError::validation("Expected an absolute local path"))?;
    let mut ancestor = path.as_path();
    loop {
        match check_directory(ancestor) {
            Ok(()) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| PathError::io(error, action))?;
            }
            Err(error) => return Err(PathError::io(error, action)),
        }
    }
    if ancestor != path {
        if !create {
            return Err(PathError::io(io::ErrorKind::NotFound.into(), action));
        }
        fs::create_dir_all(&path).map_err(|error| PathError::io(error, action))?;
        check_directory(&path).map_err(|error| PathError::io(error, action))?;
    }
    Ok(cwd.to_owned())
}

fn check_directory(path: &Path) -> io::Result<()> {
    let canonical = fs::canonicalize(path)?;
    if !canonical.metadata()?.is_dir() {
        return Err(io::ErrorKind::NotADirectory.into());
    }
    #[cfg(windows)]
    verify_windows_drive(&canonical)?;
    Ok(())
}

#[cfg(windows)]
fn verify_windows_drive(path: &Path) -> io::Result<()> {
    use std::path::Prefix;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetDriveTypeW(root: *const u16) -> u32;
    }
    let drive = match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => drive,
            Prefix::UNC(server, _) | Prefix::VerbatimUNC(server, _)
                if server.eq_ignore_ascii_case("wsl$")
                    || server.eq_ignore_ascii_case("wsl.localhost") =>
            {
                return Ok(());
            }
            _ => return Err(io::ErrorKind::InvalidInput.into()),
        },
        _ => return Err(io::ErrorKind::InvalidInput.into()),
    };
    let root = [u16::from(drive), u16::from(b':'), u16::from(b'\\'), 0];
    // The pointer is a NUL-terminated drive root and stays live for the call.
    if matches!(unsafe { GetDriveTypeW(root.as_ptr()) }, 2 | 3 | 6) {
        Ok(())
    } else {
        Err(io::ErrorKind::InvalidInput.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_expands_home_and_retains_typed_separator_and_quotes() {
        let home = Path::new(if cfg!(windows) {
            r"C:\fixture"
        } else {
            "/fixture"
        });
        let (dir, prefix, fragment, separator) =
            completion_target(home, "  '~/work/Al'  ").unwrap();
        assert_eq!(dir, home.join("work"));
        assert_eq!(
            (prefix.as_str(), fragment, separator),
            ("~/work/", "Al", '/')
        );
        assert_eq!(completion_target(home, "~").unwrap().0, home);
        assert_eq!(
            absolutize(home, "../x/./z").unwrap(),
            home.parent().unwrap().join("x/z")
        );
        assert!(absolutize(home, "C:folder").is_err());
        assert!(path_text("bad\u{7f}name").is_err());
        assert_eq!(path_text(" nonquoted ").unwrap(), " nonquoted ");
    }

    #[test]
    fn directories_complete_and_missing_child_of_file_is_not_creatable() {
        let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        fs::create_dir_all(&scratch).unwrap();
        let temp = tempfile::tempdir_in(scratch).unwrap();
        let home = temp.path();
        for name in ["zeta", "Alpha", "alpha-other"] {
            fs::create_dir(home.join(name)).unwrap();
        }
        fs::write(home.join("alpha-file"), "file").unwrap();
        let matches = complete(home, "al").unwrap();
        let mut expected = vec![format!("alpha-other{}", std::path::MAIN_SEPARATOR)];
        if cfg!(windows) {
            expected.insert(0, "Alpha\\".into());
        }
        assert_eq!(matches, expected);
        assert!(
            !resolve(home, "alpha-file/child", false)
                .unwrap_err()
                .missing
        );
        assert!(resolve(home, "new/child", false).unwrap_err().missing);
        assert!(!home.join("new").exists());
        assert!(resolve(home, "new/child", true).is_ok());
        assert!(home.join("new/child").is_dir());
    }
}
