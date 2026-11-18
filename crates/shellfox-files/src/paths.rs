use crate::{ErrorCode, FileError, Result};
use cap_std::{ambient_authority, fs::Dir};
use std::path::{Component, Path, PathBuf};

pub(crate) struct Target {
    pub parent: Dir,
    pub name: PathBuf,
    pub absolute: PathBuf,
}

pub(crate) fn validate(path: &str) -> Result<()> {
    if path.is_empty()
        || path.len() > 32767
        || path.chars().any(char::is_control)
        || path.contains("://")
        || path.starts_with("\\\\")
        || path.starts_with("//")
    {
        return Err(FileError::new(ErrorCode::Validation, "Invalid file path."));
    }
    if !cfg!(windows) && (path.contains('\\') || path.contains(':')) {
        return Err(FileError::denied());
    }
    let rest = if cfg!(windows) && path.as_bytes().get(1) == Some(&b':') {
        if !path.as_bytes()[0].is_ascii_alphabetic()
            || !matches!(path.as_bytes().get(2), Some(b'/') | Some(b'\\'))
        {
            return Err(FileError::denied());
        }
        &path[3..]
    } else {
        path
    };
    for part in rest.split(['/', '\\']) {
        let base = part.split('.').next().unwrap_or("").to_ascii_lowercase();
        let device = matches!(base.as_str(), "con" | "prn" | "aux" | "nul")
            || (base.len() == 4
                && (base.starts_with("com") || base.starts_with("lpt"))
                && matches!(base.as_bytes()[3], b'1'..=b'9'));
        if part == ".."
            || part.contains(':')
            || (cfg!(windows)
                && (device || (part != "." && (part.ends_with('.') || part.ends_with(' ')))))
        {
            return Err(FileError::denied());
        }
    }
    Ok(())
}

/// Walk one component per handle. No ancestor or final directory link is followed.
pub(crate) fn descend(dir: Dir, path: &Path) -> Result<Dir> {
    crate::directory_walk::walk(dir, path, false, false, 32767)?
        .dirs
        .pop()
        .ok_or_else(FileError::denied)
}

pub(crate) fn absolute_parts(path: &Path) -> Result<(PathBuf, PathBuf)> {
    let text = path.to_str().ok_or_else(FileError::denied)?;
    validate(text)?;
    if !path.is_absolute() {
        return Err(FileError::new(
            ErrorCode::Validation,
            "Select an absolute local directory.",
        ));
    }
    let mut components = path.components();
    let mut root = PathBuf::new();
    let first = components.next().ok_or_else(FileError::denied)?;
    match first {
        Component::Prefix(prefix) => {
            if !matches!(prefix.kind(), std::path::Prefix::Disk(_)) {
                return Err(FileError::denied());
            }
            root.push(prefix.as_os_str());
            if !matches!(components.next(), Some(Component::RootDir)) {
                return Err(FileError::denied());
            }
            root.push(std::path::MAIN_SEPARATOR.to_string());
        }
        Component::RootDir => root.push(std::path::MAIN_SEPARATOR.to_string()),
        _ => return Err(FileError::denied()),
    }
    Ok((root, components.as_path().to_path_buf()))
}
/// Syntax only. Never opens, canonicalizes, probes or enumerates the path.
pub(crate) fn validate_absolute_syntax(path: &Path) -> Result<()> {
    absolute_parts(path).map(|_| ())
}
pub(crate) fn open_absolute(path: &Path) -> Result<Dir> {
    let (root, relative) = absolute_parts(path)?;
    descend(Dir::open_ambient_dir(root, ambient_authority())?, &relative)
}

pub(crate) fn reject_link(info: &cap_std::fs::Metadata) -> Result<()> {
    if info.file_type().is_symlink() || is_reparse(info) {
        return Err(FileError::denied());
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse(info: &cap_std::fs::Metadata) -> bool {
    use cap_std::fs::MetadataExt;
    info.file_attributes() & 0x400 != 0
}
#[cfg(not(windows))]
fn is_reparse(_: &cap_std::fs::Metadata) -> bool {
    false
}

pub(crate) fn display(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(FileError::denied)
}
