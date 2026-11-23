//! Windows private objects: owner is the current user and the DACL is exactly the protected
//! owner-only DACL (plus SYSTEM for app directories). Creation passes that DACL to
//! `NtCreateFile`, so there is no window with inherited permissions.
use super::windows_acl::{
    require_private_acl, trusted_app_parent, trusted_parent as trusted, Security,
};
use super::Parent;
use crate::{FileError, Result};
use std::{
    ffi::OsStr,
    fs::File,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    ptr::null_mut,
};
use windows_sys::Win32::Foundation::UNICODE_STRING;

const READ: u32 = 0x0012_0089; // READ_CONTROL | SYNCHRONIZE | read data/EA/attributes
const DIR_RW: u32 = READ | 0x116; // + add file/subdirectory, write EA/attributes
const DIR_CREATE: u32 = 0x001e_01ff; // FILE_ALL_ACCESS without DELETE
const FILE_CREATE: u32 = 0x001f_01ff; // FILE_ALL_ACCESS; DELETE lets us rename or drop the stage
const DELETE: u32 = 0x0001_0000;
const ACL_EDIT: u32 = 0x0016_0080; // SYNCHRONIZE | READ_CONTROL | WRITE_DAC | read attributes
/// Read and write sharing only: a held directory cannot be renamed or deleted.
const SHARE_PIN: u32 = 3;
const SHARE_ALL: u32 = 7;

#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root: *mut std::ffi::c_void,
    name: *mut UNICODE_STRING,
    flags: u32,
    security: *mut std::ffi::c_void,
    qos: *mut std::ffi::c_void,
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
        file: *mut *mut std::ffi::c_void,
        access: u32,
        attributes: *const ObjectAttributes,
        status: *mut [usize; 2],
        allocation: *const i64,
        flags: u32,
        sharing: u32,
        disposition: u32,
        options: u32,
        ea: *const std::ffi::c_void,
        ea_length: u32,
    ) -> i32;
    fn RtlNtStatusToDosError(status: i32) -> u32;
}

struct Request<'a> {
    access: u32,
    sharing: u32,
    directory: bool,
    create: Option<&'a Security>,
}

/// Opens or creates one child relative to `parent`, never following a reparse point.
fn nt_open(parent: &File, name: &OsStr, request: Request<'_>) -> Result<File> {
    let mut wide: Vec<u16> = name.encode_wide().collect();
    let length = u16::try_from(wide.len() * 2).map_err(|_| FileError::denied())?;
    let mut unicode = UNICODE_STRING {
        Length: length,
        MaximumLength: length,
        Buffer: wide.as_mut_ptr(),
    };
    let attributes = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root: parent.as_raw_handle(),
        name: &mut unicode,
        flags: 0x1000, // OBJ_DONT_REPARSE
        security: request.create.map_or(null_mut(), Security::as_ptr),
        qos: null_mut(),
    };
    let (disposition, expected) = if request.create.is_some() {
        (2, 2) // FILE_CREATE -> FILE_CREATED
    } else {
        (1, 1) // FILE_OPEN -> FILE_OPENED
    };
    // Synchronous IO, open the reparse point itself, and require the expected kind.
    let options = 0x20 | 0x0020_0000 | if request.directory { 0x1 } else { 0x40 };
    let mut handle = null_mut();
    let mut status = [0usize; 2];
    // SAFETY: every pointer refers to storage that outlives this synchronous call.
    let code = unsafe {
        NtCreateFile(
            &mut handle,
            request.access,
            &attributes,
            &mut status,
            std::ptr::null(),
            if request.directory { 0x10 } else { 0x80 },
            request.sharing,
            disposition,
            options,
            std::ptr::null(),
            0,
        )
    };
    if code < 0 {
        // SAFETY: plain status conversion.
        let win32 = unsafe { RtlNtStatusToDosError(code) };
        return Err(std::io::Error::from_raw_os_error(win32 as i32).into());
    }
    // SAFETY: a successful NtCreateFile returned this owned handle.
    let file = unsafe { File::from_raw_handle(handle) };
    if code != 0 || status[1] != expected {
        return Err(FileError::new(
            crate::ErrorCode::Unsupported,
            "The file system did not confirm the private create or open.",
        ));
    }
    Ok(file)
}

fn plain(file: &File, directory: bool) -> Result<()> {
    use std::os::windows::fs::MetadataExt;
    let info = file.metadata()?;
    let kind = if directory {
        info.is_dir()
    } else {
        info.is_file() && crate::handle::link_count(file)? == 1
    };
    if !kind || info.file_attributes() & 0x400 != 0 {
        return Err(FileError::denied());
    }
    Ok(())
}

pub(super) fn trusted_parent(parent: &File, policy: Parent) -> Result<()> {
    plain(parent, true)?;
    match policy {
        Parent::Private => trusted(parent),
        Parent::App => trusted_app_parent(parent),
    }
}

pub(super) fn require_private(file: &File, directory: bool) -> Result<()> {
    plain(file, directory)?;
    require_private_acl(file, directory)
}

pub(super) fn create_dir(parent: &File, name: &OsStr, app: bool) -> Result<File> {
    let security = if app {
        Security::app_directory()?
    } else {
        Security::new(true)?
    };
    nt_open(
        parent,
        name,
        Request {
            access: DIR_CREATE,
            sharing: SHARE_PIN,
            directory: true,
            create: Some(&security),
        },
    )
}
pub(super) fn open_dir(parent: &File, name: &OsStr) -> Result<File> {
    nt_open(
        parent,
        name,
        Request {
            access: DIR_RW,
            sharing: SHARE_PIN,
            directory: true,
            create: None,
        },
    )
}
pub(super) fn open_dir_for_acl(parent: &File, name: &OsStr) -> Result<File> {
    nt_open(
        parent,
        name,
        Request {
            access: ACL_EDIT,
            sharing: SHARE_PIN,
            directory: true,
            create: None,
        },
    )
}
pub(super) fn make_private(file: &File) -> Result<()> {
    Security::app_directory()?.apply(file)
}
pub(super) fn open_file(parent: &File, name: &OsStr) -> Result<File> {
    nt_open(
        parent,
        name,
        Request {
            access: READ,
            sharing: SHARE_ALL,
            directory: false,
            create: None,
        },
    )
}
pub(super) fn open_file_for_delete(parent: &File, name: &OsStr) -> Result<File> {
    nt_open(
        parent,
        name,
        Request {
            access: READ | DELETE,
            sharing: SHARE_ALL,
            directory: false,
            create: None,
        },
    )
}
/// `deletable`: the handle itself may rename or delete the file. User-file stages are
/// renamed by a separate handle, which cannot open while this one holds DELETE.
pub(super) fn create_file(parent: &File, name: &OsStr, deletable: bool) -> Result<File> {
    let security = Security::new(false)?;
    nt_open(
        parent,
        name,
        Request {
            access: if deletable {
                FILE_CREATE
            } else {
                FILE_CREATE & !DELETE
            },
            sharing: SHARE_ALL,
            directory: false,
            create: Some(&security),
        },
    )
}

/// Renames the held object itself, so the source name cannot be swapped underneath.
pub(super) fn rename(
    parent: &File,
    _from: &OsStr,
    file: &File,
    to: &OsStr,
    replace: bool,
) -> Result<()> {
    crate::platform::rename_held_checked(file, parent, to, replace, &|| Ok(()))
}

/// Deletes the held object itself; the name is only used on Unix.
pub(super) fn remove(_parent: &File, _name: &OsStr, file: &File) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        FileDispositionInfo, SetFileInformationByHandle, FILE_DISPOSITION_INFO,
    };
    let value = FILE_DISPOSITION_INFO { DeleteFile: 1 };
    // SAFETY: the handle is live and was opened with DELETE access.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
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

/// Windows has no directory flush; rename is already atomic on NTFS.
pub(super) fn sync_dir(_: &File) -> Result<()> {
    Ok(())
}
