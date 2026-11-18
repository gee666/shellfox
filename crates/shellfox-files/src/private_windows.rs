use crate::{PrivateAccess, PrivateFileUse, PrivateKind, Result};
use std::{ffi::OsStr, fs::File};

pub(crate) fn open(
    parent: &File,
    leaf: &OsStr,
    kind: PrivateKind,
    access: PrivateAccess,
    create: Option<PrivateFileUse>,
    desired_override: Option<u32>,
    retained_failure: Option<&mut Option<File>>,
) -> Result<File> {
    open_options(
        parent,
        leaf,
        OpenPolicy {
            kind,
            access,
            create,
            desired_override,
            sharing_override: None,
        },
        retained_failure,
    )
}
pub(crate) fn open_rw_pinned_directory(parent: &File, leaf: &OsStr) -> Result<File> {
    open_options(
        parent,
        leaf,
        OpenPolicy {
            kind: PrivateKind::Directory,
            access: PrivateAccess::ReadWrite,
            create: None,
            desired_override: None,
            sharing_override: Some(3),
        },
        None,
    )
}
#[repr(C)]
pub(crate) struct Attributes {
    length: u32,
    root: *mut std::ffi::c_void,
    name: *mut windows_sys::Win32::Foundation::UNICODE_STRING,
    flags: u32,
    security: *mut std::ffi::c_void,
    qos: *mut std::ffi::c_void,
}
#[link(name = "ntdll")]
unsafe extern "system" {
    pub(crate) fn NtCreateFile(
        file: *mut *mut std::ffi::c_void,
        access: u32,
        attributes: *const Attributes,
        status: *mut [usize; 2],
        allocation: *const i64,
        flags: u32,
        sharing: u32,
        disposition: u32,
        options: u32,
        ea: *const std::ffi::c_void,
        ea_length: u32,
    ) -> i32;
}
struct OpenPolicy {
    kind: PrivateKind,
    access: PrivateAccess,
    create: Option<PrivateFileUse>,
    desired_override: Option<u32>,
    sharing_override: Option<u32>,
}
fn open_options(
    parent: &File,
    leaf: &OsStr,
    policy: OpenPolicy,
    retained_failure: Option<&mut Option<File>>,
) -> Result<File> {
    let OpenPolicy {
        kind,
        access,
        create,
        desired_override,
        sharing_override,
    } = policy;
    use crate::{ErrorCode, FileError};
    use std::{
        ffi::c_void,
        mem::size_of,
        os::windows::{
            ffi::OsStrExt,
            io::{AsRawHandle, FromRawHandle},
        },
        ptr::null_mut,
    };
    use windows_sys::Win32::Foundation::{CloseHandle, UNICODE_STRING};
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlNtStatusToDosError(status: i32) -> u32;
        fn NtQueryInformationFile(
            file: *mut c_void,
            status: *mut [usize; 2],
            value: *mut u32,
            length: u32,
            class: u32,
        ) -> i32;
    }
    let mut name: Vec<u16> = leaf.encode_wide().collect();
    let length = u16::try_from(name.len() * 2).map_err(|_| FileError::denied())?;
    let mut unicode = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: name.as_mut_ptr(),
    };
    let descriptor = if create.is_some() {
        Some(crate::private_security::Security::new(
            kind == PrivateKind::Directory,
        )?)
    } else {
        None
    };
    let attributes = Attributes {
        length: size_of::<Attributes>() as u32,
        root: parent.as_raw_handle(),
        name: &mut unicode,
        flags: 0x1000,
        security: descriptor
            .as_ref()
            .map_or(null_mut(), |value| value.as_ptr()),
        qos: null_mut(),
    };
    let exclusive =
        access == PrivateAccess::ExclusiveLease || create == Some(PrivateFileUse::ExclusiveLease);
    let sharing = sharing_override.unwrap_or(if exclusive {
        0
    } else if kind == PrivateKind::Directory && create.is_some() {
        3
    } else {
        7
    });
    // Directory handles must compose with cap-std's no-delete-sharing proof handles.
    // Keep DELETE on stages, but not on a returned directory handle.
    let desired = desired_override.unwrap_or_else(|| {
        if create.is_some() {
            if kind == PrivateKind::Directory {
                0x1e01ff
            } else {
                0x1f01ff
            }
        } else if access == PrivateAccess::ReadOnly {
            0x120089
        } else {
            0x12019f
        }
    });
    let mut handle = null_mut();
    let mut status = [0usize; 2];
    // SAFETY: all inputs remain live for this synchronous handle-relative syscall. The complete
    // protected descriptor is present at FILE_CREATE, never inherited then repaired.
    let code = unsafe {
        NtCreateFile(
            &mut handle,
            desired,
            &attributes,
            &mut status,
            std::ptr::null(),
            if kind == PrivateKind::Directory {
                0x10
            } else {
                0x80
            },
            sharing,
            if create.is_some() { 2 } else { 1 },
            0x20 | 0x200000
                | if kind == PrivateKind::Directory {
                    1
                } else {
                    0x40
                },
            std::ptr::null(),
            0,
        )
    };
    if code < 0 {
        // Safe diagnostics only: no names, paths, payloads or SIDs.
        let win32 = unsafe { RtlNtStatusToDosError(code) };
        let mut granted = 0u32;
        let mut query_status = [0usize; 2];
        // SAFETY: live original parent, correctly sized ACCESS_MASK output (FileAccessInformation).
        let queried = unsafe {
            NtQueryInformationFile(
                parent.as_raw_handle(),
                &mut query_status,
                &mut granted,
                4,
                8,
            )
        };
        let mut error = FileError::from(std::io::Error::from_raw_os_error(win32 as i32));
        error.message=format!("Private native {} {:?} failed: NTSTATUS 0x{:08x}, Win32 {}, child access 0x{:08x}, parent access query 0x{:08x}/mask 0x{:08x}.",
            if create.is_some() {"exclusive create"} else {"existing open"},kind,code as u32,win32,desired,queried as u32,granted);
        return Err(error);
    }
    if code != 0
        || handle.is_null()
        || handle as isize == -1
        || status[1] != if create.is_some() { 2 } else { 1 }
    {
        if !handle.is_null() && handle as isize != -1 {
            if let Some(slot) = retained_failure {
                // SAFETY: a successful synchronous call returned this owned handle, even though
                // its creation-evidence fields are unproven. Keep custody, never certify it.
                *slot = Some(unsafe { File::from_raw_handle(handle) });
            } else {
                unsafe {
                    CloseHandle(handle);
                }
            }
        }
        return Err(FileError::new(
            ErrorCode::Unsupported,
            "Native exclusive creation/open evidence is unavailable.",
        ));
    }
    // SAFETY: successful synchronous NtCreateFile transferred this unique owned handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}
