use super::wire::FileId;
use anyhow::{ensure, Result};
use std::{
    fs::File,
    mem,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT,
};
pub(super) fn file_id(file: &File) -> Result<FileId> {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { mem::zeroed() };
    ensure!(
        unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut info) } != 0,
        "Trusted program file identity unavailable."
    );
    ensure!(
        info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) == 0,
        "Trusted program is not a regular file."
    );
    Ok(FileId {
        first: u64::from(info.dwVolumeSerialNumber),
        second: u64::from(info.nFileIndexLow) | (u64::from(info.nFileIndexHigh) << 32),
    })
}
pub(super) fn check_id(path: &Path, expected: FileId) -> Result<()> {
    let actual = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path)?;
    ensure!(
        file_id(&actual)? == expected,
        "Trusted program file was replaced."
    );
    Ok(())
}
pub(super) fn same_file(path: &Path, held: &File) -> Result<()> {
    check_id(path, file_id(held)?)
}
