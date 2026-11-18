use super::{checked, validate_root, Fingerprint};
use crate::{Error, Result};
use rustix::fs::{open, openat, Dir, Mode, OFlags};
use std::{
    fs::File,
    os::unix::fs::{FileExt, MetadataExt},
    path::{Component, Path},
};

fn flags(directory: bool) -> OFlags {
    let base = OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK;
    if directory {
        base | OFlags::DIRECTORY
    } else {
        base
    }
}

pub(crate) fn open_root(path: &Path) -> Result<(Vec<File>, File)> {
    validate_root(path)?;
    let root: File = open("/", flags(true), Mode::empty())
        .map_err(std::io::Error::from)?
        .into();
    let mut held = vec![checked(root, true, Path::new("/"))?];
    for part in path.components() {
        match part {
            Component::RootDir => continue,
            Component::Normal(name) => {
                let parent = held
                    .last()
                    .ok_or(Error::Manifest("missing root directory"))?;
                let file: File = openat(parent, name, flags(true), Mode::empty())
                    .map_err(std::io::Error::from)?
                    .into();
                held.push(checked(file, true, path)?);
            }
            _ => return Err(Error::Manifest("ambiguous root component")),
        }
    }
    let root = held
        .last()
        .ok_or(Error::Manifest("missing root handle"))?
        .try_clone()?;
    Ok((held, root))
}

pub(crate) fn open_child(
    parent: &File,
    parent_path: &Path,
    name: &str,
    directory: bool,
) -> Result<File> {
    let file: File = openat(parent, name, flags(directory), Mode::empty())
        .map_err(std::io::Error::from)?
        .into();
    checked(file, directory, &parent_path.join(name))
}

pub(crate) fn fingerprint(file: &File) -> Result<Fingerprint> {
    let info = file.metadata()?;
    if !info.is_dir() && !info.is_file() {
        return Err(Error::Alias("not a regular file or directory".into()));
    }
    Ok(Fingerprint {
        device: info.dev(),
        id: info.ino(),
        length: info.len(),
        links: info.nlink(),
        directory: info.is_dir(),
    })
}

pub(crate) fn entries(directory: &File, _path: &Path) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let entries = Dir::read_from(directory).map_err(std::io::Error::from)?;
    for entry in entries {
        let entry = entry.map_err(std::io::Error::from)?;
        let name = entry
            .file_name()
            .to_str()
            .map_err(|_| Error::Manifest("non-Unicode payload name"))?;
        if name == "." || name == ".." {
            continue;
        }
        if result.len() >= 4096 {
            return Err(Error::Manifest("directory entry bound exceeded"));
        }
        result.push(name.to_owned());
    }
    result.sort();
    Ok(result)
}

pub(crate) fn read_at(file: &File, bytes: &mut [u8], offset: u64) -> std::io::Result<usize> {
    file.read_at(bytes, offset)
}
