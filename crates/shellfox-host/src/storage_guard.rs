//! Preflight the experiment's filesystem paths before SQLite or the lock opens them.
//! This rejects existing links, but is not a sandbox against concurrent same-user changes.
use crate::storage::{storage_error, STORE_FILENAME};
use shellfox_core::{Error, ErrorCode, Result};
use std::{
    fs::{self, File, OpenOptions},
    io,
    path::{Component, Path, PathBuf},
};

const LOCK: &str = "shellfox-rust-v1.lock";

fn absolute(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_err(storage_error)?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                let meta = fs::symlink_metadata(&normalized).map_err(storage_error)?;
                if is_link(&meta) || !meta.is_dir() || !normalized.pop() {
                    return Err(storage_error("parent component"));
                }
            }
            Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}

#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_OPEN_REPARSE_POINT,
};

fn is_link(meta: &fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

// Validate every existing component before creating the next directory, and again after.
fn directory(path: &Path, create: bool) -> Result<PathBuf> {
    let path = absolute(path)?;
    let mut current = PathBuf::new();
    for part in path.components() {
        current.push(part.as_os_str());
        if matches!(part, Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(meta) if !is_link(&meta) && meta.is_dir() => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound && create => {
                let parent = current.parent().ok_or_else(|| storage_error("no parent"))?;
                let before = parent.canonicalize().map_err(storage_error)?;
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(storage_error(e)),
                }
                let meta = fs::symlink_metadata(&current).map_err(storage_error)?;
                if is_link(&meta)
                    || !meta.is_dir()
                    || current.canonicalize().map_err(storage_error)?.parent()
                        != Some(before.as_path())
                {
                    return Err(storage_error("invalid new directory"));
                }
            }
            _ => return Err(storage_error("invalid directory")),
        }
    }
    current.canonicalize().map_err(storage_error)
}

#[cfg(unix)]
fn single_link(file: &File) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    Ok(file.metadata()?.nlink() == 1)
}

#[cfg(windows)]
fn single_link(file: &File) -> io::Result<bool> {
    use std::{mem::MaybeUninit, os::windows::io::AsRawHandle};
    let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: the handle belongs to `file` and stays open for the call.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call succeeded and filled the structure.
    let info = unsafe { info.assume_init() };
    Ok(info.nNumberOfLinks == 1 && info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT == 0)
}

#[cfg(not(any(unix, windows)))]
fn single_link(_: &File) -> io::Result<bool> {
    Ok(false)
}

fn options(write: bool) -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(write);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options
}

fn check_file(path: &Path, required: bool) -> Result<Option<File>> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound && !required => return Ok(None),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(Error::new(
                ErrorCode::NotFound,
                "Legacy database does not exist",
            ))
        }
        Err(e) => return Err(storage_error(e)),
    };
    if is_link(&meta) || !meta.is_file() {
        return Err(storage_error("invalid artifact"));
    }
    let file = options(false).open(path).map_err(storage_error)?;
    if !file.metadata().map_err(storage_error)?.is_file()
        || !single_link(&file).map_err(storage_error)?
    {
        return Err(storage_error("linked artifact"));
    }
    let parent = path.parent().ok_or_else(|| storage_error("no parent"))?;
    if path.canonicalize().map_err(storage_error)?.parent() != Some(parent) {
        return Err(storage_error("artifact outside root"));
    }
    Ok(Some(file))
}

fn artifacts(root: &Path, name: &str, required: bool) -> Result<Vec<File>> {
    let mut handles = Vec::new();
    for (i, suffix) in ["", "-wal", "-shm", "-journal"].iter().enumerate() {
        if let Some(file) = check_file(&root.join(format!("{name}{suffix}")), required && i == 0)? {
            handles.push(file);
        }
    }
    Ok(handles)
}

pub(crate) fn prepare(path: &Path) -> Result<(PathBuf, File, Vec<File>)> {
    let root = directory(path, true)?;
    let mut handles = artifacts(&root, STORE_FILENAME, false)?;
    if let Some(file) = check_file(&root.join(LOCK), false)? {
        handles.push(file);
    }
    let lock_path = root.join(LOCK);
    let file = options(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(storage_error)?;
    if !file.metadata().map_err(storage_error)?.is_file()
        || !single_link(&file).map_err(storage_error)?
    {
        return Err(storage_error("invalid lock"));
    }
    check_file(&lock_path, true)?;
    Ok((root, file, handles))
}

pub(crate) fn legacy(path: &Path) -> Result<(PathBuf, Vec<File>)> {
    let path = absolute(path)?;
    let parent = path.parent().ok_or_else(|| storage_error("no parent"))?;
    let root = directory(parent, false)?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| storage_error("invalid filename"))?;
    let handles = artifacts(&root, name, true)?;
    Ok((root.join(name), handles))
}
