//! Permission copying is handle-based and fails closed if ownership cannot be retained.
use crate::Result;
use cap_std::fs::File;

#[cfg(unix)]
pub(crate) fn preserve(previous: &File, stage: &File) -> Result<()> {
    use cap_std::fs::{MetadataExt, PermissionsExt};
    let info = previous.metadata()?;
    let staged = stage.metadata()?;
    if info.uid() == u32::MAX || info.gid() == u32::MAX {
        return Err(crate::FileError::denied());
    }
    if info.uid() != staged.uid() || info.gid() != staged.gid() {
        rustix::fs::fchown(
            stage,
            Some(rustix::fs::Uid::from_raw(info.uid())),
            Some(rustix::fs::Gid::from_raw(info.gid())),
        )
        .map_err(std::io::Error::from)?;
    }
    // Match the active service: preserve rwx bits, never copy setuid/setgid bits to edited content.
    stage.set_permissions(cap_std::fs::Permissions::from_mode(info.mode() & 0o777))?;
    copy_acl(previous, stage)?;
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn copy_acl(previous: &File, stage: &File) -> Result<()> {
    use rustix::fs::{fgetxattr, fgetxattr_query, fsetxattr, XattrFlags};
    let name = "system.posix_acl_access";
    let size = match fgetxattr_query(previous, name) {
        Ok(size) => size,
        Err(rustix::io::Errno::NODATA | rustix::io::Errno::NOTSUP) => {
            // A stage may have inherited an ACL absent on the original file.
            match rustix::fs::fremovexattr(stage, name) {
                Ok(()) | Err(rustix::io::Errno::NODATA | rustix::io::Errno::NOTSUP) => {
                    return Ok(())
                }
                Err(error) => return Err(std::io::Error::from(error).into()),
            }
        }
        Err(error) => return Err(std::io::Error::from(error).into()),
    };
    if size > 64 * 1024 {
        return Err(crate::FileError::denied());
    }
    let mut bytes = vec![0u8; size];
    let read = fgetxattr(previous, name, bytes.as_mut_slice()).map_err(std::io::Error::from)?;
    fsetxattr(stage, name, &bytes[..read], XattrFlags::empty()).map_err(std::io::Error::from)?;
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn copy_acl(previous: &File, stage: &File) -> Result<()> {
    use std::{ffi::c_void, os::fd::AsRawFd};
    // Darwin's ACL API has no portable libc declaration. These signatures match sys/acl.h.
    unsafe extern "C" {
        fn acl_get_fd_np(fd: i32, kind: i32) -> *mut c_void;
        fn acl_set_fd_np(fd: i32, acl: *mut c_void, kind: i32) -> i32;
        fn acl_free(acl: *mut c_void) -> i32;
        fn acl_delete_fd_np(fd: i32, kind: i32) -> i32;
    }
    const ACL_TYPE_EXTENDED: i32 = 0x100;
    // SAFETY: previous is live, and acl is released exactly once after setting on the live stage.
    unsafe {
        let acl = acl_get_fd_np(previous.as_raw_fd(), ACL_TYPE_EXTENDED);
        if acl.is_null() {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ENOENT) {
                if acl_delete_fd_np(stage.as_raw_fd(), ACL_TYPE_EXTENDED) == 0 {
                    return Ok(());
                }
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ENOENT) {
                    return Ok(());
                }
                return Err(error.into());
            }
            return Err(error.into());
        }
        let result = acl_set_fd_np(stage.as_raw_fd(), acl, ACL_TYPE_EXTENDED);
        let error = std::io::Error::last_os_error();
        acl_free(acl);
        if result != 0 {
            return Err(error.into());
        }
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn same_owner(previous: &File, stage: &File) -> Result<()> {
    use std::{os::windows::io::AsRawHandle, ptr::null_mut};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
            EqualSid, OWNER_SECURITY_INFORMATION,
        },
    };
    let mut owners = [null_mut(); 2];
    let mut descriptors = [null_mut(); 2];
    let mut result = Ok(());
    for (index, file) in [previous, stage].into_iter().enumerate() {
        // SAFETY: all output pointers are valid and descriptor owns the returned SID.
        let code = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owners[index],
                null_mut(),
                null_mut(),
                null_mut(),
                &mut descriptors[index],
            )
        };
        if code != 0 {
            result = Err(std::io::Error::from_raw_os_error(code as i32).into());
            break;
        }
    }
    // SAFETY: non-null owners are owned by the still-live descriptors.
    if result.is_ok()
        && (owners.iter().any(|owner| owner.is_null())
            || unsafe { EqualSid(owners[0], owners[1]) } == 0)
    {
        result = Err(crate::FileError::denied());
    }
    for descriptor in descriptors {
        if !descriptor.is_null() {
            // SAFETY: each descriptor comes from GetSecurityInfo and is freed exactly once.
            unsafe {
                LocalFree(descriptor);
            }
        }
    }
    result
}
