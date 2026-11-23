use crate::{Error, Result};
use shellfox_files::handle;
use std::{fs::File, path::Path};

pub(crate) fn directory_handle(path: &Path) -> Result<File> {
    let file = open_directory(path)?;
    check(&file, true)?;
    Ok(file)
}
/// Identity pin only, never app-private directory authority.
pub(crate) fn identity_parent_handle(path: &Path) -> Result<File> {
    let file = open_directory(path)?;
    let info = file.metadata()?;
    if !info.is_dir() || handle::is_link(&info) {
        return Err(Error::Permissions);
    }
    Ok(file)
}
fn open_directory(path: &Path) -> Result<File> {
    let mut options = handle::nofollow_options();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Read and write sharing only: the directory cannot be renamed or deleted while held.
        options.share_mode(3);
    }
    options.open(path).map_err(|_| Error::Permissions)
}

/// A checked external identity is read-only, with no write/delete sharing on
/// Windows. Store handles keep their existing independent policy.
pub(crate) fn identity_file_handle(path: &Path) -> Result<File> {
    let mut options = handle::nofollow_options();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1); // read sharing only
    }
    let file = options.open(path).map_err(|_| Error::Permissions)?;
    check(&file, false)?;
    Ok(file)
}

pub(crate) fn check(file: &File, directory: bool) -> Result<()> {
    let metadata = file.metadata()?;
    let kind = if directory {
        metadata.is_dir()
    } else {
        handle::is_single_link_file(file)?
    };
    if !kind || handle::is_link(&metadata) {
        return Err(Error::Permissions);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid has no preconditions and returns the process identity.
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & if directory { 0o077 } else { 0o177 } != 0
        {
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
    Ok(handle::same_file(a, b)?)
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
