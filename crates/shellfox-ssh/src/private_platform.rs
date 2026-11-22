use crate::{Error, Result};
use std::{
    fs::{File, OpenOptions},
    path::Path,
};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_READ, FILE_SHARE_WRITE,
};

pub(crate) fn directory_handle(path: &Path) -> Result<File> {
    let file = open_directory(path)?;
    check(&file, true)?;
    Ok(file)
}
/// Identity pin only, never app-private directory authority.
pub(crate) fn identity_parent_handle(path: &Path) -> Result<File> {
    let file = open_directory(path)?;
    if !file.metadata()?.is_dir() {
        return Err(Error::Permissions);
    }
    #[cfg(windows)]
    if information(&file)?.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(Error::Permissions);
    }
    Ok(file)
}
fn open_directory(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Keep this directory pinned against rename/delete while private state is open.
        options
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    }
    options.open(path).map_err(|_| Error::Permissions)
}

/// A checked external identity is read-only, with no write/delete sharing on
/// Windows. Store handles keep their existing independent policy.
pub(crate) fn identity_file_handle(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .share_mode(FILE_SHARE_READ);
    }
    let file = options.open(path).map_err(|_| Error::Permissions)?;
    check(&file, false)?;
    Ok(file)
}

pub(crate) fn check(file: &File, directory: bool) -> Result<()> {
    let metadata = file.metadata()?;
    if if directory {
        !metadata.is_dir()
    } else {
        !metadata.is_file()
    } {
        return Err(Error::Permissions);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid has no preconditions and returns the process identity.
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & if directory { 0o077 } else { 0o177 } != 0
            || (!directory && metadata.nlink() != 1)
        {
            return Err(Error::Permissions);
        }
    }
    #[cfg(windows)]
    {
        let info = information(file)?;
        if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || (!directory && info.nNumberOfLinks != 1) {
            return Err(Error::Permissions);
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        return Err(Error::Permissions);
    }
    Ok(())
}

pub(crate) fn same(a: &File, b: &File) -> Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let (a, b) = (a.metadata()?, b.metadata()?);
        Ok((a.dev(), a.ino()) == (b.dev(), b.ino()))
    }
    #[cfg(windows)]
    {
        let (a, b) = (information(a)?, information(b)?);
        Ok((a.dwVolumeSerialNumber, a.nFileIndexHigh, a.nFileIndexLow)
            == (b.dwVolumeSerialNumber, b.nFileIndexHigh, b.nFileIndexLow))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (a, b);
        Err(Error::Permissions)
    }
}

#[cfg(windows)]
fn information(
    file: &File,
) -> Result<windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: file owns a live handle; the API writes exactly this structure on success.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
        return Err(Error::Permissions);
    }
    // SAFETY: the successful call initialized info.
    Ok(unsafe { info.assume_init() })
}

pub(crate) fn sync_directory(file: &File) -> Result<()> {
    #[cfg(unix)]
    {
        file.sync_all()?;
    }
    // Windows rename is atomic, but std provides no portable directory flush guarantee.
    #[cfg(not(unix))]
    {
        let _ = file;
    }
    Ok(())
}
