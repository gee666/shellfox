use super::super::{error, SystemResult};
use shellfox_protocol::ErrorCode;
use std::fs::File;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn copy_attributes(source: &File, target: &File) -> SystemResult<()> {
    use std::{
        ffi::{c_void, CString},
        os::fd::AsRawFd,
    };
    #[cfg(target_os = "linux")]
    extern "C" {
        fn flistxattr(fd: i32, names: *mut i8, size: usize) -> isize;
        fn fgetxattr(fd: i32, name: *const i8, value: *mut c_void, size: usize) -> isize;
        fn fsetxattr(
            fd: i32,
            name: *const i8,
            value: *const c_void,
            size: usize,
            flags: i32,
        ) -> i32;
    }
    #[cfg(target_os = "macos")]
    extern "C" {
        fn flistxattr(fd: i32, names: *mut i8, size: usize, options: i32) -> isize;
        fn fgetxattr(
            fd: i32,
            name: *const i8,
            value: *mut c_void,
            size: usize,
            position: u32,
            options: i32,
        ) -> isize;
        fn fsetxattr(
            fd: i32,
            name: *const i8,
            value: *const c_void,
            size: usize,
            position: u32,
            options: i32,
        ) -> i32;
    }
    #[cfg(target_os = "macos")]
    copy_macos_acl(source, target)?;
    // SAFETY: descriptor queries; every buffer below is allocated to the bounded
    // OS-reported size. No attribute value is logged or passed to a command.
    #[cfg(target_os = "linux")]
    let size = unsafe { flistxattr(source.as_raw_fd(), std::ptr::null_mut(), 0) };
    #[cfg(target_os = "macos")]
    let size = unsafe { flistxattr(source.as_raw_fd(), std::ptr::null_mut(), 0, 0) };
    if size < 0 {
        let code = std::io::Error::last_os_error().raw_os_error();
        #[cfg(target_os = "linux")]
        if code == Some(95) {
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        if code == Some(45) {
            return Ok(());
        }
        return Err(storage());
    }
    if size > 65536 {
        return Err(storage());
    }
    let mut names = vec![0u8; size as usize];
    #[cfg(target_os = "linux")]
    let actual = unsafe { flistxattr(source.as_raw_fd(), names.as_mut_ptr().cast(), names.len()) };
    #[cfg(target_os = "macos")]
    let actual = unsafe {
        flistxattr(
            source.as_raw_fd(),
            names.as_mut_ptr().cast(),
            names.len(),
            0,
        )
    };
    if actual != size {
        return Err(storage());
    }
    let mut total = 0;
    for raw in names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let name = CString::new(raw).map_err(|_| storage())?;
        #[cfg(target_os = "linux")]
        let length =
            unsafe { fgetxattr(source.as_raw_fd(), name.as_ptr(), std::ptr::null_mut(), 0) };
        #[cfg(target_os = "macos")]
        let length = unsafe {
            fgetxattr(
                source.as_raw_fd(),
                name.as_ptr(),
                std::ptr::null_mut(),
                0,
                0,
                0,
            )
        };
        if !(0..=1024 * 1024).contains(&length) {
            return Err(storage());
        }
        total += length;
        if total > 1024 * 1024 {
            return Err(storage());
        }
        let mut bytes = vec![0u8; length as usize];
        #[cfg(target_os = "linux")]
        let count = unsafe {
            fgetxattr(
                source.as_raw_fd(),
                name.as_ptr(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
            )
        };
        #[cfg(target_os = "macos")]
        let count = unsafe {
            fgetxattr(
                source.as_raw_fd(),
                name.as_ptr(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                0,
                0,
            )
        };
        if count != length {
            return Err(storage());
        }
        #[cfg(target_os = "linux")]
        let result = unsafe {
            fsetxattr(
                target.as_raw_fd(),
                name.as_ptr(),
                bytes.as_ptr().cast(),
                bytes.len(),
                0,
            )
        };
        #[cfg(target_os = "macos")]
        let result = unsafe {
            fsetxattr(
                target.as_raw_fd(),
                name.as_ptr(),
                bytes.as_ptr().cast(),
                bytes.len(),
                0,
                0,
            )
        };
        if result != 0 {
            return Err(storage());
        }
    }
    Ok(())
}
#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn copy_attributes(_: &File, _: &File) -> SystemResult<()> {
    Err(error(
        ErrorCode::Unsupported,
        "Permission-preserving installation is not implemented for this OS.",
    ))
}
#[cfg(target_os = "macos")]
fn copy_macos_acl(source: &File, target: &File) -> SystemResult<()> {
    use std::{ffi::c_void, os::fd::AsRawFd};
    extern "C" {
        fn acl_get_fd(fd: i32) -> *mut c_void;
        fn acl_set_fd(fd: i32, acl: *mut c_void) -> i32;
        fn acl_size(acl: *mut c_void) -> isize;
        fn acl_free(acl: *mut c_void) -> i32;
    }
    // SAFETY: owned live descriptors; ACL allocation is returned by libc and
    // freed on every branch. Apply only to this owned staging file.
    let acl = unsafe { acl_get_fd(source.as_raw_fd()) };
    if acl.is_null() {
        if std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
            return shellfox_transport::tighten_held_acl(target).map_err(|_| storage());
        }
        return Err(storage());
    }
    let size = unsafe { acl_size(acl) };
    let result = if (0..=65536).contains(&size) {
        unsafe { acl_set_fd(target.as_raw_fd(), acl) }
    } else {
        -1
    };
    unsafe {
        acl_free(acl);
    }
    if result != 0 {
        return Err(storage());
    }
    Ok(())
}
fn storage() -> shellfox_protocol::AppError {
    error(
        ErrorCode::StorageFailed,
        "File ACL/extended attributes cannot be preserved safely.",
    )
}
