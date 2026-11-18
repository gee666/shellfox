use super::{checked, validate_root, Fingerprint};
use crate::{Error, Result};
use std::{
    fs::{self, File, OpenOptions},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::{FileExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Component, Path, PathBuf, Prefix},
};
use windows_sys::Win32::Storage::FileSystem::{
    GetDriveTypeW, GetFileInformationByHandle, GetFinalPathNameByHandleW,
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, VOLUME_NAME_GUID,
};

fn open(path: &Path, directory: bool) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    checked(file, directory, path)
}

pub(crate) fn open_root(path: &Path) -> Result<(Vec<File>, File)> {
    validate_root(path)?;
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Err(Error::Manifest("local drive root required"));
    };
    if !matches!(prefix.kind(), Prefix::Disk(_)) || components.next() != Some(Component::RootDir) {
        return Err(Error::Manifest(
            "UNC and device namespace roots are not supported",
        ));
    }
    let mut current = PathBuf::from(prefix.as_os_str());
    current.push("\\");
    let wide: Vec<u16> = current.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: wide is a live NUL-terminated root path. Only local removable,
    // fixed, CD-ROM and RAM disks are allowed, not mapped remote servers whose
    // behavior cannot be constrained by local file-sharing guarantees.
    let drive_type = unsafe { GetDriveTypeW(wide.as_ptr()) };
    if !matches!(drive_type, 2 | 3 | 5 | 6) {
        return Err(Error::Manifest("local filesystem bundle root required"));
    }
    let mut held = vec![open(&current, true)?];
    for part in components {
        let Component::Normal(name) = part else {
            return Err(Error::Manifest("ambiguous root component"));
        };
        let text = name
            .to_str()
            .ok_or(Error::Manifest("root must be Unicode"))?;
        if text.ends_with(['.', ' ']) || text.contains(':') {
            return Err(Error::Manifest("Win32 root alias"));
        }
        current.push(name);
        held.push(open(&current, true)?);
    }
    let root = held
        .last()
        .ok_or(Error::Manifest("missing root handle"))?
        .try_clone()?;
    let physical = final_path(&root)?;
    let ancestors: Vec<_> = physical.ancestors().collect();
    for parent in ancestors.into_iter().rev() {
        held.push(open(parent, true)?);
    }
    let physical_root = held
        .last()
        .ok_or(Error::Manifest("physical root handle missing"))?;
    let original = fingerprint(&root)?;
    let resolved = fingerprint(physical_root)?;
    if original.device != resolved.device || original.id != resolved.id {
        return Err(Error::Changed(
            "root mapping changed during verification".into(),
        ));
    }
    Ok((held, root))
}

pub(crate) fn open_child(
    _parent: &File,
    parent_path: &Path,
    name: &str,
    directory: bool,
) -> Result<File> {
    // Every ancestor directory already has a no-delete-sharing handle. No
    // component is replaced between this path lookup and the held file open.
    open(&parent_path.join(name), directory)
}

pub(crate) fn fingerprint(file: &File) -> Result<Fingerprint> {
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: file owns a live handle; the API initializes this exact output type.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), info.as_mut_ptr()) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: successful GetFileInformationByHandle initialized the structure.
    let info = unsafe { info.assume_init() };
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(Error::Alias("reparse point".into()));
    }
    Ok(Fingerprint {
        device: u64::from(info.dwVolumeSerialNumber),
        id: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        length: (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow),
        links: u64::from(info.nNumberOfLinks),
        directory: info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0,
    })
}

pub(crate) fn entries(_directory: &File, path: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(path)? {
        if names.len() >= 4096 {
            return Err(Error::Manifest("directory entry bound exceeded"));
        }
        names.push(
            entry?
                .file_name()
                .into_string()
                .map_err(|_| Error::Manifest("non-Unicode payload name"))?,
        );
    }
    names.sort();
    Ok(names)
}

pub(crate) fn final_path(file: &File) -> Result<PathBuf> {
    let mut buffer = vec![0u16; 1024];
    loop {
        // SAFETY: handle is owned; writable buffer capacity matches the API length.
        let count = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle().cast(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                VOLUME_NAME_GUID,
            )
        };
        if count == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if count as usize >= buffer.len() {
            if count > 16384 {
                return Err(Error::Manifest("resolved native path exceeds bound"));
            }
            buffer.resize(count as usize + 1, 0);
            continue;
        }
        let path = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..count as usize]));
        if !path.to_string_lossy().starts_with("\\\\?\\Volume{") {
            return Err(Error::Manifest("expected an OS-derived local volume path"));
        }
        return Ok(path);
    }
}

pub(crate) fn read_at(file: &File, bytes: &mut [u8], offset: u64) -> std::io::Result<usize> {
    file.seek_read(bytes, offset)
}
