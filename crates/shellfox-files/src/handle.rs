//! Handle checks shared by every crate that opens app-owned files: never follow a link or
//! reparse point, refuse hardlinked files, and compare full native object identity.
use std::{
    fs::{File, Metadata, OpenOptions},
    io,
};

/// Full native object identity: volume plus 128-bit file ID on Windows, device plus inode on
/// Unix. Not serializable, so it cannot be forged from a request.
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
    pub fn of(file: &File) -> io::Result<Self> {
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
            // SAFETY: live handle and correctly sized output, read only after success.
            if unsafe {
                GetFileInformationByHandleEx(
                    file.as_raw_handle(),
                    FileIdInfo,
                    info.as_mut_ptr().cast(),
                    std::mem::size_of::<FILE_ID_INFO>() as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: the call succeeded and filled the structure.
            let info = unsafe { info.assume_init() };
            if info.FileId.Identifier.iter().all(|byte| *byte == 0) {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "file system reports no file ID",
                ));
            }
            Ok(Self {
                volume: info.VolumeSerialNumber,
                file: info.FileId.Identifier,
            })
        }
    }
    pub fn from_file(file: &File) -> crate::Result<Self> {
        Ok(Self::of(file)?)
    }
    pub(crate) fn from_cap_file(file: &cap_std::fs::File) -> crate::Result<Self> {
        Self::from_file(&file.try_clone()?.into_std())
    }
    pub(crate) fn from_directory(directory: &cap_std::fs::Dir) -> crate::Result<Self> {
        // Duplicate the held handle; never reopen the path.
        Self::from_file(&directory.try_clone()?.into_std_file())
    }
    pub fn matches(&self, file: &File) -> crate::Result<bool> {
        Ok(self == &Self::from_file(file)?)
    }
}

/// Both handles refer to the same file system object.
pub fn same_file(a: &File, b: &File) -> io::Result<bool> {
    Ok(HandleIdentity::of(a)? == HandleIdentity::of(b)?)
}

/// A symlink, or on Windows any reparse point (junction, mount point, cloud placeholder).
pub fn is_link(info: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if info.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    info.file_type().is_symlink()
}

/// Number of hard links to an open file.
pub fn link_count(file: &File) -> io::Result<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(file.metadata()?.nlink())
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        // SAFETY: live handle; the call writes this structure on success.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the call succeeded and filled the structure.
        Ok(u64::from(unsafe { info.assume_init() }.nNumberOfLinks))
    }
}

/// Options that open the named object itself: O_NOFOLLOW on Unix, the reparse point itself
/// on Windows (plus backup semantics so directories open). Check `is_link` on the result.
pub fn nofollow_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS
        options.custom_flags(0x0020_0000 | 0x0200_0000);
    }
    options
}

/// An opened regular file with exactly one link that is not a link or reparse point.
pub fn is_single_link_file(file: &File) -> io::Result<bool> {
    let info = file.metadata()?;
    Ok(info.is_file() && !is_link(&info) && link_count(file)? == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{link_dir, TempDir};

    #[test]
    fn links_hardlinks_and_identity_are_detected_on_handles() {
        let dir = TempDir::new();
        std::fs::write(dir.join("a"), b"x").unwrap();
        let open = |name: &str| nofollow_options().read(true).open(dir.join(name)).unwrap();
        let a = open("a");
        assert!(is_single_link_file(&a).unwrap());
        assert!(same_file(&a, &open("a")).unwrap());
        std::fs::hard_link(dir.join("a"), dir.join("b")).unwrap();
        assert_eq!(link_count(&a).unwrap(), 2);
        assert!(!is_single_link_file(&open("b")).unwrap());
        std::fs::create_dir(dir.join("target")).unwrap();
        link_dir(&dir.join("target"), &dir.join("link"));
        assert!(is_link(
            &std::fs::symlink_metadata(dir.join("link")).unwrap()
        ));
        #[cfg(windows)]
        assert!(
            is_link(&open("link").metadata().unwrap()),
            "the junction itself opens"
        );
        #[cfg(unix)]
        assert!(nofollow_options()
            .read(true)
            .open(dir.join("link"))
            .is_err());
        assert!(!same_file(&a, &open("target")).unwrap());
    }
}
