use crate::{paths, ErrorCode, FileError, Result, TEXT_BYTES};
use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, Metadata, OpenOptions};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

pub(crate) fn same_held_file(a: &File, b: &File) -> Result<bool> {
    Ok(crate::HandleIdentity::from_cap_file(a)? == crate::HandleIdentity::from_cap_file(b)?)
}
pub(crate) struct Snapshot {
    pub metadata: Metadata,
    pub identity: crate::HandleIdentity,
    changed: (i64, i64),
}

pub(crate) fn same_version(a: &Snapshot, b: &Snapshot) -> bool {
    a.identity == b.identity
        && a.metadata.len() == b.metadata.len()
        && a.metadata.modified().ok() == b.metadata.modified().ok()
        && a.metadata.permissions() == b.metadata.permissions()
        && a.changed == b.changed
}

pub(crate) fn snapshot(file: &File) -> Result<Snapshot> {
    let identity = crate::HandleIdentity::from_cap_file(file)?;
    let metadata = file.metadata()?;
    #[cfg(unix)]
    let changed = {
        use cap_std::fs::MetadataExt;
        (metadata.ctime(), metadata.ctime_nsec())
    };
    #[cfg(windows)]
    let changed = {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FileBasicInfo, GetFileInformationByHandleEx, FILE_BASIC_INFO,
        };
        let mut info = std::mem::MaybeUninit::<FILE_BASIC_INFO>::uninit();
        // SAFETY: live handle, correctly sized output buffer, initialized only after success.
        let code = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileBasicInfo,
                info.as_mut_ptr().cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        };
        if code == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: successful GetFileInformationByHandleEx initialized every field.
        (unsafe { info.assume_init() }.ChangeTime, 0)
    };
    Ok(Snapshot {
        metadata,
        identity,
        changed,
    })
}

pub(crate) fn open_regular(dir: &Dir, name: &Path) -> Result<File> {
    let info = dir.symlink_metadata(name)?;
    paths::reject_link(&info)?;
    if !info.is_file() {
        return Err(FileError::new(
            ErrorCode::Unsupported,
            "Only regular files can be read.",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = dir.open_with(name, &options)?;
    let opened = file.metadata()?;
    paths::reject_link(&opened)?;
    if !opened.is_file() {
        return Err(FileError::new(
            ErrorCode::Unsupported,
            "Only regular files can be read.",
        ));
    }
    Ok(file)
}

pub(crate) fn read(file: &mut File, limit: usize) -> Result<(Vec<u8>, Snapshot)> {
    let before = snapshot(file)?;
    if before.metadata.len() > limit as u64 {
        return Err(FileError::new(
            ErrorCode::Unsupported,
            "File exceeds the preview or editor limit.",
        ));
    }
    let mut bytes = Vec::with_capacity((before.metadata.len() as usize).min(limit));
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit
        || bytes.len() as u64 != before.metadata.len()
        || !same_version(&before, &snapshot(file)?)
    {
        return Err(FileError::conflict());
    }
    Ok((bytes, before))
}

pub(crate) fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn text(bytes: &[u8]) -> Result<String> {
    if bytes.len() > TEXT_BYTES || bytes.contains(&0) {
        return Err(FileError::new(
            ErrorCode::Unsupported,
            "Only UTF-8 text without NUL up to 128 KiB can be edited.",
        ));
    }
    String::from_utf8(bytes.to_vec())
        .map_err(|_| FileError::new(ErrorCode::Unsupported, "This file is not UTF-8 text."))
}

pub(crate) fn validate_text(value: &str) -> Result<()> {
    if value.len() > TEXT_BYTES || value.contains('\0') {
        return Err(FileError::new(
            ErrorCode::Validation,
            "Text exceeds 128 KiB or contains NUL.",
        ));
    }
    Ok(())
}

pub(crate) fn validate_revision(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(FileError::new(
            ErrorCode::Validation,
            "Invalid file revision.",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "content_snapshot_tests.rs"]
mod snapshot_tests;

pub(crate) fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[255, 216, 255]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else if bytes.starts_with(&[0, 0, 1, 0]) {
        Some("image/x-icon")
    } else if bytes.get(4..8) == Some(b"ftyp")
        && matches!(bytes.get(8..12), Some(b"avif") | Some(b"avis"))
    {
        Some("image/avif")
    } else {
        None
    }
}
