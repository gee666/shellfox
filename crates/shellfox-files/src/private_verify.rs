use crate::{FileError, PrivateIdentity, PrivateKind, Result};
use std::fs::File;

pub(crate) fn kind(file: &File, expected: PrivateKind) -> Result<()> {
    let info = file.metadata()?;
    if if expected == PrivateKind::Directory {
        !info.is_dir()
    } else {
        !info.is_file()
    } {
        return Err(FileError::denied());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if info.file_attributes() & 0x400 != 0 {
            return Err(FileError::denied());
        }
    }
    if expected == PrivateKind::File && links(file)? != 1 {
        return Err(FileError::denied());
    }
    Ok(())
}
pub(crate) fn parent(file: &File) -> Result<PrivateIdentity> {
    kind(file, PrivateKind::Directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let info = file.metadata()?;
        // SAFETY: geteuid has no arguments or ownership requirements.
        let user = unsafe { libc::geteuid() };
        if (info.uid() != user && info.uid() != 0) || info.mode() & 0o022 != 0 {
            return Err(FileError::denied());
        }
        darwin_acl(file)?;
    }
    #[cfg(windows)]
    crate::private_parent_windows::verify(file)?;
    PrivateIdentity::from_file(file)
}
pub(crate) fn object(file: &File, expected: PrivateKind) -> Result<PrivateIdentity> {
    kind(file, expected)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let info = file.metadata()?;
        // SAFETY: geteuid has no arguments or ownership requirements.
        if info.uid() != unsafe { libc::geteuid() }
            || info.mode() & 0o7777
                != if expected == PrivateKind::Directory {
                    0o700
                } else {
                    0o600
                }
        {
            return Err(FileError::denied());
        }
        darwin_acl(file)?;
    }
    #[cfg(windows)]
    {
        crate::private_security::Security::new(expected == PrivateKind::Directory)?
            .require_owner(file)?;
        let file = cap_std::fs::File::from_std(file.try_clone()?);
        crate::journal_private::fingerprint_kind(&file, expected == PrivateKind::Directory)?;
    }
    PrivateIdentity::from_file(file)
}
#[cfg(unix)]
fn darwin_acl(file: &File) -> Result<()> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        let handle = cap_std::fs::File::from_std(file.try_clone()?);
        crate::journal_private::require_empty_darwin_acl(&handle).map_err(|_| {
            FileError::new(
                crate::ErrorCode::Unsupported,
                "Darwin private creation requires proven empty access ACL inheritance.",
            )
        })?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    let _ = file;
    Ok(())
}
pub(crate) fn links(file: &File) -> Result<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(file.metadata()?.nlink())
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        // SAFETY: the live handle writes this correctly sized structure on success.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(unsafe { info.assume_init() }.nNumberOfLinks as u64)
    }
}
