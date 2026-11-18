use crate::{PrivateAccess, PrivateFileUse, PrivateKind, Result};
use std::{ffi::OsStr, fs::File};

#[cfg(unix)]
pub(crate) fn create(
    parent: &File,
    leaf: &OsStr,
    kind: PrivateKind,
    _: PrivateFileUse,
) -> Result<File> {
    use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
    use cap_std::fs::{Dir, DirBuilder, DirBuilderExt, OpenOptions, OpenOptionsExt};
    let directory = Dir::from_std_file(parent.try_clone()?);
    if kind == PrivateKind::Directory {
        let mut builder = DirBuilder::new();
        builder.mode(0o700);
        directory.create_dir_with(leaf, &builder)?;
        Ok(directory.open_dir_nofollow(leaf)?.into_std_file())
    } else {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK)
            .follow(FollowSymlinks::No);
        Ok(directory.open_with(leaf, &options)?.into_std())
    }
}
#[cfg(unix)]
pub(crate) fn open(
    parent: &File,
    leaf: &OsStr,
    kind: PrivateKind,
    access: PrivateAccess,
) -> Result<File> {
    use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
    use cap_std::fs::{Dir, OpenOptions, OpenOptionsExt};
    let directory = Dir::from_std_file(parent.try_clone()?);
    if kind == PrivateKind::Directory {
        return Ok(directory.open_dir_nofollow(leaf)?.into_std_file());
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(access != PrivateAccess::ReadOnly)
        .custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK)
        .follow(FollowSymlinks::No);
    Ok(directory.open_with(leaf, &options)?.into_std())
}
// Native-return inspection wrapper only; production uses create_retained with error custody.
#[cfg(all(windows, test))]
pub(crate) fn create(
    parent: &File,
    leaf: &OsStr,
    kind: PrivateKind,
    purpose: PrivateFileUse,
) -> Result<File> {
    windows_open(
        parent,
        leaf,
        kind,
        PrivateAccess::ReadWrite,
        Some(purpose),
        None,
        None,
    )
}
#[cfg(windows)]
pub(crate) fn open(
    parent: &File,
    leaf: &OsStr,
    kind: PrivateKind,
    access: PrivateAccess,
) -> Result<File> {
    windows_open(parent, leaf, kind, access, None, None, None)
}

#[cfg(windows)]
use crate::private_windows::open as windows_open;

pub(crate) fn create_retained(
    parent: &File,
    leaf: &OsStr,
    kind: PrivateKind,
    purpose: PrivateFileUse,
    slot: &mut Option<File>,
) -> Result<()> {
    #[cfg(unix)]
    {
        *slot = Some(create(parent, leaf, kind, purpose)?);
        Ok(())
    }
    #[cfg(windows)]
    {
        let file = windows_open(
            parent,
            leaf,
            kind,
            PrivateAccess::ReadWrite,
            Some(purpose),
            None,
            Some(slot),
        )?;
        *slot = Some(file);
        Ok(())
    }
}

/// Explicit authorized native-directory reopen; the adapter checks original logical RW/fence.
pub(crate) fn open_rw_pinned_directory(parent: &File, leaf: &OsStr) -> Result<File> {
    #[cfg(windows)]
    {
        crate::private_windows::open_rw_pinned_directory(parent, leaf)
    }
    #[cfg(unix)]
    {
        open(
            parent,
            leaf,
            PrivateKind::Directory,
            PrivateAccess::ReadWrite,
        )
    }
}

pub(crate) fn lock(file: &File) -> Result<()> {
    #[cfg(unix)]
    rustix::fs::flock(file, rustix::fs::FlockOperation::NonBlockingLockExclusive)
        .map_err(std::io::Error::from)?;
    #[cfg(windows)]
    let _ = file; // exclusive sharing was acquired by NtCreateFile
    Ok(())
}

pub(crate) fn remove(parent: &File, leaf: &OsStr, file: &File, kind: PrivateKind) -> Result<()> {
    #[cfg(unix)]
    {
        let _ = file;
        let directory = cap_std::fs::Dir::from_std_file(parent.try_clone()?);
        if kind == PrivateKind::Directory {
            directory.remove_dir(leaf)?;
        } else {
            directory.remove_file(leaf)?;
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FileDispositionInfo, SetFileInformationByHandle, FILE_DISPOSITION_INFO,
        };
        let reopened = if kind == PrivateKind::Directory {
            // Descriptor-relative delete access, checked against the CREATED full identity.
            // Never obtain cleanup authority by reopening an absolute pathname.
            let held = windows_open(
                parent,
                leaf,
                kind,
                PrivateAccess::ReadOnly,
                None,
                Some(0x110080),
                None,
            )?;
            if !crate::PrivateIdentity::from_file(file)?.matches(&held)? {
                return Err(crate::FileError::conflict());
            }
            Some(held)
        } else {
            None
        };
        let handle = reopened
            .as_ref()
            .map_or(file.as_raw_handle(), |value| value.as_raw_handle());
        let value = FILE_DISPOSITION_INFO { DeleteFile: 1 };
        // SAFETY: created handle owns DELETE access; this deletes only that object, not a rebound name.
        if unsafe {
            SetFileInformationByHandle(
                handle,
                FileDispositionInfo,
                (&value as *const FILE_DISPOSITION_INFO).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
}
