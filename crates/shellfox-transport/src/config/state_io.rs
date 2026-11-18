use crate::HelperError;
#[cfg(not(unix))]
use std::fs::OpenOptions;
use std::{fs::File, path::Path};

pub(super) fn directory(path: &Path) -> Result<File, HelperError> {
    #[cfg(unix)]
    let file = {
        use rustix::fs::{open, Mode, OFlags};
        File::from(
            open(
                path,
                OFlags::RDONLY
                    | OFlags::DIRECTORY
                    | OFlags::NOFOLLOW
                    | OFlags::CLOEXEC
                    | OFlags::NONBLOCK,
                Mode::empty(),
            )
            .map_err(|_| HelperError::PermissionDenied)?,
        )
    };
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        OpenOptions::new()
            .read(true)
            .share_mode(3)
            .custom_flags(0x02000000 | 0x00200000) // BACKUP_SEMANTICS | OPEN_REPARSE_POINT
            .open(path)
            .map_err(|_| HelperError::PermissionDenied)?
    };
    #[cfg(not(any(unix, windows)))]
    let file = File::open(path).map_err(|_| HelperError::PermissionDenied)?;
    validate_kind(&file, true)?;
    Ok(file)
}
pub(super) fn create_marker(directory: &File, path: &Path) -> Result<File, HelperError> {
    marker(directory, path, true)
}
pub(super) fn open_marker(directory: &File, path: &Path) -> Result<File, HelperError> {
    marker(directory, path, false)
}
fn marker(directory: &File, path: &Path, create: bool) -> Result<File, HelperError> {
    #[cfg(unix)]
    let result = {
        use rustix::fs::{openat, Mode, OFlags};
        let name = path.file_name().ok_or(HelperError::InvalidRequest)?;
        let flags = OFlags::NOFOLLOW
            | OFlags::CLOEXEC
            | OFlags::NONBLOCK
            | if create {
                OFlags::CREATE | OFlags::EXCL | OFlags::RDWR
            } else {
                OFlags::RDONLY
            };
        openat(directory, name, flags, Mode::from_raw_mode(0o600))
            .map(File::from)
            .map_err(std::io::Error::from)
    };
    #[cfg(not(unix))]
    let result = {
        let _ = directory;
        let mut options = OpenOptions::new();
        options.read(true);
        if create {
            options.write(true).create_new(true);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // Pin the exact marker against replacement or writes during proof.
            // Go's metadata/DACL-only verifier still opens this same file.
            options.share_mode(1).custom_flags(0x00200000);
        }
        options.open(path)
    };
    let file = result.map_err(|error| {
        if !create && error.kind() == std::io::ErrorKind::NotFound {
            HelperError::StateUnmarked
        } else {
            HelperError::PermissionDenied
        }
    })?;
    validate_marker(&file)?;
    Ok(file)
}
fn validate_kind(file: &File, directory: bool) -> Result<(), HelperError> {
    let info = file.metadata().map_err(|_| HelperError::PermissionDenied)?;
    if (if directory {
        !info.is_dir()
    } else {
        !info.is_file()
    }) || info.file_type().is_symlink()
    {
        return Err(HelperError::PermissionDenied);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if info.file_attributes() & 0x400 != 0 {
            return Err(HelperError::PermissionDenied);
        }
    }
    Ok(())
}
pub(super) fn validate_marker(file: &File) -> Result<(), HelperError> {
    validate_kind(file, false)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let info = file.metadata().map_err(|_| HelperError::PermissionDenied)?;
        crate::unix_acl::verify_held_acl(file)?;
        if info.nlink() != 1
            || info.uid() != rustix::process::getuid().as_raw()
            || info.mode() & 0o177 != 0
        {
            return Err(HelperError::PermissionDenied);
        }
    }
    // Windows owner, protected DACL and single-link proof comes from the pinned
    // verified Go helper, not portable metadata or a caller-supplied boolean.
    Ok(())
}
