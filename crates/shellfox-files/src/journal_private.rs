//! Fingerprint an effective owner-only journal directory, not its pathname.
use crate::{FileError, Result};
use cap_std::fs::File;

#[cfg(unix)]
pub(crate) fn fingerprint(file: &File) -> Result<Vec<u8>> {
    use cap_std::fs::MetadataExt;
    let info = file.metadata()?;
    // SAFETY: geteuid has no arguments or ownership requirements.
    if info.uid() != unsafe { libc::geteuid() } || info.mode() & 0o7777 != 0o700 {
        return Err(FileError::denied());
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    require_empty_darwin_acl(file)?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&info.uid().to_le_bytes());
    bytes.extend_from_slice(&info.gid().to_le_bytes());
    bytes.extend_from_slice(&(info.mode() & 0o7777).to_le_bytes());
    Ok(bytes)
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) fn require_empty_darwin_acl(file: &File) -> Result<()> {
    use std::{ffi::c_void, os::fd::AsRawFd};
    unsafe extern "C" {
        fn acl_get_fd_np(fd: i32, kind: i32) -> *mut c_void;
        fn acl_get_entry(acl: *mut c_void, kind: i32, entry: *mut *mut c_void) -> i32;
        fn acl_free(acl: *mut c_void) -> i32;
    }
    // chmod alone does not neutralize Darwin access ACLs. Decline unproven ACLs.
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
        acl_free(acl);
        if found != 0 {
            return Err(FileError::denied());
        }
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn fingerprint(file: &File) -> Result<Vec<u8>> {
    fingerprint_kind(file, true)
}

#[cfg(windows)]
pub(crate) fn fingerprint_kind(file: &File, directory: bool) -> Result<Vec<u8>> {
    use std::{os::windows::io::AsRawHandle, ptr::null_mut};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
            EqualSid, GetAce, GetLengthSid, GetSecurityDescriptorControl, IsWellKnownSid,
            WinLocalSystemSid, ACCESS_ALLOWED_ACE, DACL_SECURITY_INFORMATION,
            OWNER_SECURITY_INFORMATION, SE_DACL_PROTECTED,
        },
    };
    let mut owner = null_mut();
    let mut acl = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: all output storage is valid. Descriptor owns the returned SID and ACL.
    let code = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut acl,
            null_mut(),
            &mut descriptor,
        )
    };
    if code != 0 {
        return Err(std::io::Error::from_raw_os_error(code as i32).into());
    }
    let result = (|| {
        if owner.is_null() || acl.is_null() {
            return Err(FileError::denied());
        }
        let mut control = 0;
        let mut revision = 0;
        let mut ace = null_mut();
        // SAFETY: descriptor, ACL and SID stay live through the final copy.
        unsafe {
            if GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) == 0
                || control & SE_DACL_PROTECTED == 0
                || !matches!((*acl).AceCount, 1 | 2)
                || GetAce(acl, 0, &mut ace) == 0
            {
                return Err(FileError::denied());
            }
            let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            if ace.Header.AceType != 0
                || ace.Header.AceFlags != if directory { 3 } else { 0 }
                || ace.Mask != 0x1f01ff
                || EqualSid(owner, (&ace.SidStart as *const u32).cast_mut().cast()) == 0
            {
                return Err(FileError::denied());
            }
            if (*acl).AceCount == 2 {
                let mut system = null_mut();
                let current = crate::private_security::current_sid()?;
                if !directory
                    || EqualSid(owner, current.as_ptr().cast_mut().cast()) == 0
                    || GetAce(acl, 1, &mut system) == 0
                {
                    return Err(FileError::denied());
                }
                let system = &*system.cast::<ACCESS_ALLOWED_ACE>();
                if system.Header.AceType != 0
                    || system.Header.AceFlags != 0
                    || system.Mask != 0x1f01ff
                    || IsWellKnownSid(
                        (&system.SidStart as *const u32).cast_mut().cast(),
                        WinLocalSystemSid,
                    ) == 0
                {
                    return Err(FileError::denied());
                }
            }
            let size = GetLengthSid(owner) as usize;
            if size == 0 || size > 68 {
                return Err(FileError::denied());
            }
            let mut fingerprint = std::slice::from_raw_parts(owner.cast::<u8>(), size).to_vec();
            fingerprint.extend_from_slice(&(*acl).AceCount.to_le_bytes());
            Ok(fingerprint)
        }
    })();
    // SAFETY: GetSecurityInfo allocated descriptor and it is freed exactly once.
    unsafe {
        LocalFree(descriptor);
    }
    result
}
