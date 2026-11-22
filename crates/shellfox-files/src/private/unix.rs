//! Unix private objects: owned by the effective user, mode exactly 0700 or 0600.
//! POSIX mkdir returns no handle, so a new directory is reopened with O_NOFOLLOW and
//! verified; that is safe only because the parent is not writable by anyone else.
use super::Parent;
use crate::{FileError, HandleIdentity, Result};
use rustix::fs::{Mode, OFlags};
use std::{ffi::OsStr, fs::File, os::unix::fs::MetadataExt};

fn euid() -> u32 {
    // SAFETY: geteuid has no arguments and cannot fail.
    unsafe { libc::geteuid() }
}

fn open(parent: &File, name: &OsStr, flags: OFlags, mode: Mode) -> Result<File> {
    let fd = rustix::fs::openat(
        parent,
        name,
        flags | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        mode,
    )
    .map_err(std::io::Error::from)?;
    Ok(File::from(fd))
}

pub(super) fn trusted_parent(parent: &File, _: Parent) -> Result<()> {
    let info = parent.metadata()?;
    if !info.is_dir() || (info.uid() != euid() && info.uid() != 0) || info.mode() & 0o022 != 0 {
        return Err(FileError::denied());
    }
    darwin_acl(parent)
}

pub(super) fn require_private(file: &File, directory: bool) -> Result<()> {
    let info = file.metadata()?;
    let (kind, mode) = if directory {
        (info.is_dir(), 0o700)
    } else {
        (info.is_file() && info.nlink() == 1, 0o600)
    };
    if !kind || info.uid() != euid() || info.mode() & 0o7777 != mode {
        return Err(FileError::denied());
    }
    darwin_acl(file)
}

/// chmod does not neutralize Darwin access ACLs, so any entry is refused.
#[cfg(any(target_os = "macos", target_os = "ios"))]
fn darwin_acl(file: &File) -> Result<()> {
    use std::{ffi::c_void, os::fd::AsRawFd};
    unsafe extern "C" {
        fn acl_get_fd_np(fd: i32, kind: i32) -> *mut c_void;
        fn acl_get_entry(acl: *mut c_void, kind: i32, entry: *mut *mut c_void) -> i32;
        fn acl_free(acl: *mut c_void) -> i32;
    }
    // SAFETY: the descriptor is live; an allocated ACL is freed exactly once.
    unsafe {
        let acl = acl_get_fd_np(file.as_raw_fd(), 0x100);
        if acl.is_null() {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ENOENT) {
                return Ok(());
            }
            return Err(error.into());
        }
        let mut entry = std::ptr::null_mut();
        let found = acl_get_entry(acl, 0, &mut entry);
        let error = std::io::Error::last_os_error();
        acl_free(acl);
        // Darwin returns 0 when an entry exists and EINVAL for an empty ACL.
        if found == 0 || error.raw_os_error() != Some(libc::EINVAL) {
            return Err(FileError::denied());
        }
    }
    Ok(())
}
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn darwin_acl(_: &File) -> Result<()> {
    Ok(())
}

pub(super) fn create_dir(parent: &File, name: &OsStr, _app: bool) -> Result<File> {
    rustix::fs::mkdirat(parent, name, Mode::from_raw_mode(0o700)).map_err(std::io::Error::from)?;
    open_dir(parent, name)
}
pub(super) fn open_dir(parent: &File, name: &OsStr) -> Result<File> {
    open(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY,
        Mode::empty(),
    )
}
pub(super) fn open_dir_for_acl(parent: &File, name: &OsStr) -> Result<File> {
    open_dir(parent, name)
}
pub(super) fn make_private(file: &File) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if file.metadata()?.uid() != euid() {
        return Err(FileError::denied());
    }
    file.set_permissions(std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}
pub(super) fn open_file(parent: &File, name: &OsStr) -> Result<File> {
    // O_NONBLOCK: a FIFO raced into place cannot hang the reader.
    open(
        parent,
        name,
        OFlags::RDONLY | OFlags::NONBLOCK,
        Mode::empty(),
    )
}
pub(super) fn open_file_for_delete(parent: &File, name: &OsStr) -> Result<File> {
    open_file(parent, name)
}
pub(super) fn create_file(parent: &File, name: &OsStr) -> Result<File> {
    open(
        parent,
        name,
        OFlags::RDWR | OFlags::CREATE | OFlags::EXCL,
        Mode::from_raw_mode(0o600),
    )
}

fn names(parent: &File, name: &OsStr, file: &File) -> Result<()> {
    let named = rustix::fs::statat(parent, name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW)
        .map_err(std::io::Error::from)?;
    let info = file.metadata()?;
    if named.st_dev as u64 != info.dev() || named.st_ino as u64 != info.ino() {
        return Err(FileError::conflict());
    }
    Ok(())
}

/// Renames the temp file `from` (still bound to `file`) to `to` in the same directory.
pub(super) fn rename(
    parent: &File,
    from: &OsStr,
    file: &File,
    to: &OsStr,
    replace: bool,
) -> Result<()> {
    names(parent, from, file)?;
    if replace {
        rustix::fs::renameat(parent, from, parent, to)
    } else {
        rustix::fs::renameat_with(parent, from, parent, to, rustix::fs::RenameFlags::NOREPLACE)
    }
    .map_err(std::io::Error::from)?;
    Ok(())
}

/// Unlinks `name` only while it still names `file`.
pub(super) fn remove(parent: &File, name: &OsStr, file: &File) -> Result<()> {
    names(parent, name, file)?;
    rustix::fs::unlinkat(parent, name, rustix::fs::AtFlags::empty())
        .map_err(std::io::Error::from)?;
    Ok(())
}

/// fsync through a readable descriptor for the same directory; cap-std's may be O_PATH.
pub(super) fn sync_dir(parent: &File) -> Result<()> {
    let readable = open(
        parent,
        OsStr::new("."),
        OFlags::RDONLY | OFlags::DIRECTORY,
        Mode::empty(),
    )?;
    if HandleIdentity::from_file(&readable)? != HandleIdentity::from_file(parent)? {
        return Err(FileError::conflict());
    }
    readable.sync_all()?;
    Ok(())
}
