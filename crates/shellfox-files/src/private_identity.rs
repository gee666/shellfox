use crate::Result;
#[cfg(windows)]
use crate::{ErrorCode, FileError};
use std::fs::File;

/// Full native object identity. No public constructor, serialization or u64 Windows projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandleIdentity {
    #[cfg(windows)]
    volume: u64,
    #[cfg(windows)]
    file: [u8; 16],
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}
impl HandleIdentity {
    pub(crate) fn from_cap_file(file: &cap_std::fs::File) -> Result<Self> {
        let original = file.try_clone()?.into_std();
        Self::from_file(&original)
    }
    pub(crate) fn from_directory(directory: &cap_std::fs::Dir) -> Result<Self> {
        // Duplicate the SAME original native object, never reopen its path.
        let original = directory.try_clone()?.into_std_file();
        Self::from_file(&original)
    }
    pub fn from_file(file: &File) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let info = file.metadata()?;
            Ok(Self {
                device: info.dev(),
                inode: info.ino(),
            })
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::{
                FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO,
            };
            let mut info = std::mem::MaybeUninit::<FILE_ID_INFO>::uninit();
            // SAFETY: live handle, correctly sized output storage, read only after success.
            if unsafe {
                GetFileInformationByHandleEx(
                    file.as_raw_handle(),
                    FileIdInfo,
                    info.as_mut_ptr().cast(),
                    std::mem::size_of::<FILE_ID_INFO>() as u32,
                )
            } == 0
            {
                return Err(FileError::new(
                    ErrorCode::Unsupported,
                    "Full native file identity is unavailable.",
                ));
            }
            let info = unsafe { info.assume_init() };
            if info.FileId.Identifier.iter().all(|byte| *byte == 0) {
                return Err(FileError::new(
                    ErrorCode::Unsupported,
                    "Native file identity is unproven.",
                ));
            }
            Ok(Self {
                volume: info.VolumeSerialNumber,
                file: info.FileId.Identifier,
            })
        }
    }
    pub fn matches(&self, file: &File) -> Result<bool> {
        Ok(self == &Self::from_file(file)?)
    }
}
#[cfg(all(test, windows))]
mod tests {
    use super::*;
    // Equality mechanics only: no native query/ReFS acceptance claim.
    #[test]
    fn mechanical_windows_file_id_high_bits_are_not_projected() {
        let original = HandleIdentity {
            volume: 1,
            file: [7; 16],
        };
        let mut other = original.clone();
        other.file[15] ^= 1;
        assert_ne!(original, other);
    }
    #[test]
    fn mechanical_windows_volume_high_bits_are_not_projected() {
        let original = HandleIdentity {
            volume: 1,
            file: [7; 16],
        };
        let mut other = original.clone();
        other.volume |= 1_u64 << 63;
        assert_ne!(original, other);
    }
}
