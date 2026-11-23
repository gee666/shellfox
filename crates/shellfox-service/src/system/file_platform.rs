use super::{error, io::FileRevision, SystemResult};
use shellfox_protocol::ErrorCode;
#[cfg(not(windows))]
use std::fs;
use std::{fs::File, path::Path};

#[cfg(windows)]
pub(super) fn identity(file: &File) -> SystemResult<(u64, u64, u32, i128)> {
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    #[derive(Default)]
    struct Info {
        attributes: u32,
        creation: [u32; 2],
        access: [u32; 2],
        write: [u32; 2],
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct Basic {
        creation: i64,
        access: i64,
        write: i64,
        change: i64,
        attributes: u32,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandle(file: *mut std::ffi::c_void, info: *mut Info) -> i32;
        fn GetFileInformationByHandleEx(
            file: *mut std::ffi::c_void,
            class: u32,
            info: *mut std::ffi::c_void,
            size: u32,
        ) -> i32;
    }
    let (mut info, mut basic) = (Info::default(), Basic::default());
    // SAFETY: live owned handle; output layouts match the documented Win32 structures.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0
        || info.links != 1
        || unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                0,
                (&mut basic as *mut Basic).cast(),
                std::mem::size_of::<Basic>() as u32,
            )
        } == 0
    {
        return Err(error(
            ErrorCode::AuthFailed,
            "System file identity/hardlink validation failed.",
        ));
    }
    Ok((
        u64::from(info.volume),
        (u64::from(info.index_high) << 32) | u64::from(info.index_low),
        info.attributes,
        i128::from(basic.change),
    ))
}
#[cfg(unix)]
pub(super) fn identity(file: &File) -> SystemResult<(u64, u64, u32, i128)> {
    use std::os::unix::fs::MetadataExt;
    let info = file.metadata().map_err(|_| storage())?;
    Ok((
        info.dev(),
        info.ino(),
        info.mode(),
        i128::from(info.ctime()) * 1_000_000_000 + i128::from(info.ctime_nsec()),
    ))
}
pub(super) fn validate_mutation(path: &Path, expected: Option<&FileRevision>) -> SystemResult<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        extern "C" {
            fn geteuid() -> u32;
        }
        if expected.is_some() {
            let info = fs::metadata(path).map_err(|_| storage())?;
            // SAFETY: a side-effect-free current effective UID query.
            if info.uid() != unsafe { geteuid() } {
                return Err(error(
                    ErrorCode::AuthFailed,
                    "Per-user integration file is owned by another account.",
                ));
            }
        }
    }
    #[cfg(not(unix))]
    let _ = (path, expected);
    Ok(())
}
pub(super) fn prepare_permissions(
    file: &File,
    path: &Path,
    expected: &FileRevision,
    executable: bool,
) -> SystemResult<()> {
    let original = File::open(path).map_err(|_| storage())?;
    if &super::io::file_revision(&original)? != expected {
        return Err(error(
            ErrorCode::FileConflict,
            "File permissions changed before staging.",
        ));
    }
    let info = original.metadata().map_err(|_| storage())?;
    #[cfg(unix)]
    let mut permissions = info.permissions();
    #[cfg(not(unix))]
    let permissions = info.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::{
            fs::{MetadataExt, PermissionsExt},
            io::AsRawFd,
        };
        extern "C" {
            fn fchown(fd: i32, uid: u32, gid: u32) -> i32;
        }
        // SAFETY: only the owned staging descriptor is changed. If owner/group
        // cannot be preserved, fail rather than broadening access.
        if unsafe { fchown(file.as_raw_fd(), info.uid(), info.gid()) } != 0 {
            return Err(storage());
        }
        if executable {
            permissions.set_mode(permissions.mode() | 0o100);
        }
    }
    #[cfg(not(unix))]
    let _ = executable;
    file.set_permissions(permissions).map_err(|_| storage())?;
    #[cfg(unix)]
    copy_attributes(&original, file)?;
    Ok(())
}
#[cfg(windows)]
pub(super) fn commit_existing(target: &Path, staged: &Path) -> SystemResult<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn ReplaceFileW(
            target: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void,
        ) -> i32;
    }
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    let staged: Vec<u16> = staged.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: owned NUL-terminated paths. ReplaceFile preserves target DACL and
    // extended metadata; merge errors are not ignored.
    if unsafe {
        ReplaceFileW(
            target.as_ptr(),
            staged.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(storage());
    }
    Ok(())
}
#[cfg(not(windows))]
pub(super) fn commit_existing(target: &Path, staged: &Path) -> SystemResult<()> {
    fs::rename(staged, target).map_err(|_| storage())
}

#[cfg(unix)]
mod xattrs;
#[cfg(unix)]
use xattrs::copy_attributes;

fn storage() -> shellfox_protocol::AppError {
    error(
        ErrorCode::StorageFailed,
        "Permission-preserving integration staging/commit failed; no automatic retry.",
    )
}
