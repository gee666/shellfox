use crate::{paths::Target, Result};
use cap_std::fs::File;

#[cfg(windows)]
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtSetInformationFile(
        file: *mut std::ffi::c_void,
        status: *mut std::ffi::c_void,
        information: *const std::ffi::c_void,
        length: u32,
        class: u32,
    ) -> i32;
    fn RtlNtStatusToDosError(status: i32) -> u32;
}

#[cfg(unix)]
pub(crate) fn rename_new(source: &Target, destination: &Target) -> Result<()> {
    rustix::fs::renameat_with(
        &source.parent,
        &source.name,
        &destination.parent,
        &destination.name,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(std::io::Error::from)?;
    Ok(())
}

#[cfg(windows)]
pub(crate) fn rename_new(source: &Target, destination: &Target) -> Result<()> {
    rename_checked(source, destination, false, None, &|| Ok(()), false)
}

pub(crate) fn rename_user_new_checked(
    source: &Target,
    destination: &Target,
    expected: &File,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    #[cfg(windows)]
    {
        rename_checked(source, destination, false, Some(expected), check, true)
    }
    #[cfg(unix)]
    {
        let current = crate::content::open_regular(&source.parent, &source.name)?;
        if !crate::content::same_held_file(&current, expected)? {
            return Err(crate::FileError::conflict());
        }
        check()?;
        rename_new(source, destination)
    }
}
pub(crate) fn rename_replace(source: &Target, destination: &Target, expected: &File) -> Result<()> {
    rename_replace_inner(source, destination, expected, &|| Ok(()), false)
}
pub(crate) fn rename_user_replace_checked(
    source: &Target,
    destination: &Target,
    expected: &File,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    rename_replace_inner(source, destination, expected, check, true)
}
fn rename_replace_inner(
    source: &Target,
    destination: &Target,
    expected: &File,
    check: &dyn Fn() -> Result<()>,
    retained_destination: bool,
) -> Result<()> {
    #[cfg(windows)]
    {
        rename_checked(
            source,
            destination,
            true,
            Some(expected),
            check,
            retained_destination,
        )
    }
    #[cfg(unix)]
    {
        let current = crate::content::open_regular(&source.parent, &source.name)?;
        if !crate::content::same_held_file(&current, expected)? {
            return Err(crate::FileError::conflict());
        }
        let _ = retained_destination;
        check()?;
        rustix::fs::renameat(
            &source.parent,
            &source.name,
            &destination.parent,
            &destination.name,
        )
        .map_err(std::io::Error::from)?;
        Ok(())
    }
}

#[cfg(windows)]
fn rename_checked(
    source: &Target,
    destination: &Target,
    replace: bool,
    expected: Option<&File>,
    check: &dyn Fn() -> Result<()>,
    retained_destination: bool,
) -> Result<()> {
    use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
    use cap_std::fs::{OpenOptions, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_READ_ATTRIBUTES};
    let mut options = OpenOptions::new();
    options
        .access_mode(DELETE | FILE_READ_ATTRIBUTES)
        .follow(FollowSymlinks::No)
        .maybe_dir(true);
    let file = source.parent.open_with(&source.name, &options)?;
    crate::paths::reject_link(&file.metadata()?)?;
    if let Some(expected) = expected {
        if !crate::content::same_held_file(&file, expected)? {
            return Err(crate::FileError::conflict());
        }
    }
    rename_held_information(
        &file.try_clone()?.into_std(),
        &destination.parent.try_clone()?.into_std_file(),
        destination.name.as_os_str(),
        replace,
        check,
        retained_destination,
    )
}

#[cfg(windows)]
pub(crate) fn rename_held_checked(
    file: &std::fs::File,
    parent: &std::fs::File,
    leaf: &std::ffi::OsStr,
    replace: bool,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    rename_held_information(file, parent, leaf, replace, check, false)
}
/// Keep the expected destination FD alive through user-file CAS. POSIX replacement is
/// required on Windows when an existing destination has retained readers. No legacy retry.
#[cfg(windows)]
pub(crate) fn rename_user_stage_held_checked(
    file: &std::fs::File,
    parent: &std::fs::File,
    leaf: &std::ffi::OsStr,
    replace: bool,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    rename_held_information(file, parent, leaf, replace, check, true)
}
#[cfg(windows)]
fn rename_held_information(
    file: &std::fs::File,
    parent: &std::fs::File,
    leaf: &std::ffi::OsStr,
    replace: bool,
    check: &dyn Fn() -> Result<()>,
    retained_destination: bool,
) -> Result<()> {
    use std::{
        mem::{offset_of, size_of},
        os::windows::{ffi::OsStrExt, io::AsRawHandle},
    };
    use windows_sys::Win32::Storage::FileSystem::FILE_RENAME_INFO;
    let name: Vec<u16> = leaf.encode_wide().collect();
    // The native call requires at least the full struct, including its trailing
    // padding, even when a one-character leaf fits in fewer bytes.
    let length = (offset_of!(FILE_RENAME_INFO, FileName) + name.len() * 2)
        .max(size_of::<FILE_RENAME_INFO>());
    // usize storage supplies the alignment required by FILE_RENAME_INFO.
    let mut buffer = vec![0usize; length.div_ceil(size_of::<usize>())];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: buffer is aligned, zeroed and large enough for the header and UTF-16 name.
    // Both source and destination are pinned handles and name is one validated component.
    unsafe {
        if retained_destination {
            (*info).Anonymous.Flags = if replace { 3 } else { 0 };
        } else {
            (*info).Anonymous.ReplaceIfExists = u8::from(replace);
        }
        (*info).RootDirectory = parent.as_raw_handle();
        (*info).FileNameLength = (name.len() * 2) as u32;
        std::ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
        // Win32's FileRenameInfo rejects a non-null RootDirectory. Use the native
        // handle-relative form instead of converting either directory back to a string.
        let mut status = [0usize; 2];
        check()?;
        let code = NtSetInformationFile(
            file.as_raw_handle(),
            status.as_mut_ptr().cast(),
            info.cast(),
            length as u32,
            if retained_destination { 65 } else { 10 },
        );
        if code < 0 {
            return Err(
                std::io::Error::from_raw_os_error(RtlNtStatusToDosError(code) as i32).into(),
            );
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn stage_options() -> cap_std::fs::OpenOptions {
    use cap_std::fs::OpenOptionsExt;
    let mut options = cap_std::fs::OpenOptions::new();
    options.read(true).write(true).create_new(true).mode(0o600);
    options
}

#[cfg(windows)]
pub(crate) fn stage_options() -> cap_std::fs::OpenOptions {
    use cap_std::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::WRITE_DAC;
    let mut options = cap_std::fs::OpenOptions::new();
    // READ_CONTROL is included in GENERIC_READ. The stage owner can set its DACL.
    options
        .read(true)
        .write(true)
        .create_new(true)
        .access_mode(0x80000000 | 0x40000000 | WRITE_DAC);
    options
}

#[cfg(unix)]
pub(crate) fn preserve_permissions(previous: &File, stage: &File) -> Result<()> {
    crate::permissions::preserve(previous, stage)
}

#[cfg(windows)]
pub(crate) fn preserve_permissions(previous: &File, stage: &File) -> Result<()> {
    crate::permissions::same_owner(previous, stage)?;
    use std::{os::windows::io::AsRawHandle, ptr::null_mut};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{GetSecurityInfo, SetSecurityInfo, SE_FILE_OBJECT},
            GetSecurityDescriptorControl, DACL_SECURITY_INFORMATION,
            PROTECTED_DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED,
            UNPROTECTED_DACL_SECURITY_INFORMATION,
        },
    };
    let mut dacl = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: the source handle is live; GetSecurityInfo allocates descriptor, which owns dacl.
    let code = unsafe {
        GetSecurityInfo(
            previous.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if code != 0 {
        return Err(std::io::Error::from_raw_os_error(code as i32).into());
    }
    let mut control = 0;
    let mut revision = 0;
    // SAFETY: descriptor and dacl remain valid until the LocalFree below.
    let result = unsafe {
        if GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            let protection = if control & SE_DACL_PROTECTED != 0 {
                PROTECTED_DACL_SECURITY_INFORMATION
            } else {
                UNPROTECTED_DACL_SECURITY_INFORMATION
            };
            let code = SetSecurityInfo(
                stage.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | protection,
                null_mut(),
                null_mut(),
                dacl,
                null_mut(),
            );
            if code == 0 {
                Ok(())
            } else {
                Err(std::io::Error::from_raw_os_error(code as i32))
            }
        }
    };
    // SAFETY: descriptor was allocated by GetSecurityInfo and is freed exactly once.
    unsafe {
        LocalFree(descriptor);
    }
    result?;
    // Preserve the read-only attribute, too. A read-only destination is rejected before staging.
    stage.set_permissions(previous.metadata()?.permissions())?;
    Ok(())
}
