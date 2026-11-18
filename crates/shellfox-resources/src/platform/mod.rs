use crate::{Error, Result};
use std::{fs::File, path::Path};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::*;
#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Fingerprint {
    pub device: u64,
    pub id: u64,
    pub length: u64,
    pub links: u64,
    pub directory: bool,
}

pub(crate) fn checked(file: File, directory: bool, name: &Path) -> Result<File> {
    let metadata = fingerprint(&file)?;
    if metadata.directory != directory || (!directory && metadata.links != 1) {
        return Err(Error::Alias(name.display().to_string()));
    }
    Ok(file)
}

pub(crate) fn validate_root(path: &Path) -> Result<()> {
    let text = path
        .to_str()
        .ok_or(Error::Manifest("root must be valid Unicode"))?;
    if !path.is_absolute()
        || text.len() > 8192
        || text.chars().any(char::is_control)
        || text
            .split(['/', '\\'])
            .any(|part| part == "." || part == "..")
    {
        return Err(Error::Manifest(
            "explicit unambiguous absolute bundle root required",
        ));
    }
    Ok(())
}
