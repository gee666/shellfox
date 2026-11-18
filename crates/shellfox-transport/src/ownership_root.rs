use crate::{normalize_private_path, HelperError};
use std::{
    fs::File,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, PartialEq, Eq)]
pub struct OwnershipRootIdentity(String);
impl std::fmt::Debug for OwnershipRootIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OwnershipRootIdentity([held])")
    }
}
impl OwnershipRootIdentity {
    pub(crate) fn from_file(file: &File) -> Result<Self, HelperError> {
        let info = file.metadata().map_err(|_| HelperError::PermissionDenied)?;
        if !info.is_dir() {
            return Err(HelperError::InvalidRequest);
        }
        #[cfg(target_os = "linux")]
        {
            let info = rustix::fs::fstat(file).map_err(|_| HelperError::UnsupportedIdentity)?;
            Ok(format_linux(info.st_dev, info.st_ino))
        }
        #[cfg(target_os = "macos")]
        {
            let info = rustix::fs::fstat(file).map_err(|_| HelperError::UnsupportedIdentity)?;
            let bits = u32::from_ne_bytes(info.st_dev.to_ne_bytes());
            Ok(format_darwin(bits, info.st_ino))
        }
        #[cfg(windows)]
        {
            windows_identity(file)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            Err(HelperError::UnsupportedIdentity)
        }
    }
    pub(crate) fn argument(&self) -> &str {
        &self.0
    }
}
struct Bound {
    path: PathBuf,
    _file: File,
    _ancestors: Vec<File>,
    identity: OwnershipRootIdentity,
}
/// Owns the original root descriptor, not an identity re-captured from its path.
#[derive(Clone)]
pub struct BoundOwnershipRoot(Arc<Bound>);
impl std::fmt::Debug for BoundOwnershipRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BoundOwnershipRoot([held])")
    }
}
impl BoundOwnershipRoot {
    pub fn from_held(path: PathBuf, original: File) -> Result<Self, HelperError> {
        let path = normalize_private_path(&path)?;
        if path.parent().is_none() {
            return Err(HelperError::InvalidRequest);
        }
        let identity = OwnershipRootIdentity::from_file(&original)?;
        Ok(Self(Arc::new(Bound {
            path,
            _file: original,
            _ancestors: Vec::new(),
            identity,
        })))
    }
    pub fn path(&self) -> &Path {
        &self.0.path
    }
    pub fn identity(&self) -> &OwnershipRootIdentity {
        &self.0.identity
    }
    pub(crate) fn directory_components(&self) -> usize {
        1 + self
            .0
            .path
            .components()
            .filter(|part| matches!(part, Component::Normal(_)))
            .count()
    }
    pub(crate) fn original_files(&self) -> Result<Vec<File>, HelperError> {
        self.0
            ._ancestors
            .iter()
            .chain(std::iter::once(&self.0._file))
            .map(|file| file.try_clone().map_err(|_| HelperError::PermissionDenied))
            .collect()
    }
    #[cfg(unix)]
    pub(crate) fn held_file(&self) -> &File {
        &self.0._file
    }
    pub(crate) fn capture_current(path: &Path) -> Result<Self, HelperError> {
        let path = normalize_private_path(path)?;
        let mut base = PathBuf::new();
        let mut parts = path.components();
        match parts.next() {
            #[cfg(windows)]
            Some(Component::Prefix(p)) => {
                base.push(p.as_os_str());
                if !matches!(parts.next(), Some(Component::RootDir)) {
                    return Err(HelperError::InvalidRequest);
                }
                base.push(std::path::MAIN_SEPARATOR.to_string());
            }
            Some(Component::RootDir) => base.push(std::path::MAIN_SEPARATOR.to_string()),
            _ => return Err(HelperError::InvalidRequest),
        }
        let mut handles = vec![open_directory(&base, None)?];
        for component in parts {
            let Component::Normal(name) = component else {
                return Err(HelperError::InvalidRequest);
            };
            if handles.len() >= 256 {
                return Err(HelperError::InvalidRequest);
            }
            base.push(name);
            handles.push(open_directory(
                &base,
                handles.last().map(|parent| (parent, name)),
            )?);
        }
        let file = handles.pop().ok_or(HelperError::InvalidRequest)?;
        let identity = OwnershipRootIdentity::from_file(&file)?;
        Ok(Self(Arc::new(Bound {
            path,
            _file: file,
            _ancestors: handles,
            identity,
        })))
    }
}
fn open_directory(
    path: &Path,
    parent: Option<(&File, &std::ffi::OsStr)>,
) -> Result<File, HelperError> {
    #[cfg(unix)]
    {
        use rustix::fs::{open, openat, Mode, OFlags};
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let result = if let Some((parent, name)) = parent {
            openat(parent, name, flags, Mode::empty())
        } else {
            open(path, flags, Mode::empty())
        };
        result
            .map(File::from)
            .map_err(|_| HelperError::PermissionDenied)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        let _ = parent;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .custom_flags(0x02000000 | 0x00200000)
            .open(path)
            .map_err(|_| HelperError::PermissionDenied)?;
        let info = file.metadata().map_err(|_| HelperError::PermissionDenied)?;
        if !info.is_dir() || info.file_attributes() & 0x400 != 0 {
            return Err(HelperError::PermissionDenied);
        }
        Ok(file)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (path, parent);
        Err(HelperError::UnsupportedIdentity)
    }
}
#[cfg(any(target_os = "linux", test))]
fn format_linux(device: u64, inode: u64) -> OwnershipRootIdentity {
    OwnershipRootIdentity(format!("root-v1:linux:{device:016x}:{inode:016x}"))
}
#[cfg(any(target_os = "macos", test))]
fn format_darwin(bits: u32, inode: u64) -> OwnershipRootIdentity {
    OwnershipRootIdentity(format!("root-v1:darwin:{bits:08x}:{inode:016x}"))
}
#[cfg(any(windows, test))]
fn format_windows(volume: u64, id: [u8; 16]) -> Result<OwnershipRootIdentity, HelperError> {
    if id == [0; 16] {
        return Err(HelperError::UnsupportedIdentity);
    }
    let id: String = id.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(OwnershipRootIdentity(format!(
        "root-v1:windows:{volume:016x}:{id}"
    )))
}

#[cfg(windows)]
fn windows_identity(file: &File) -> Result<OwnershipRootIdentity, HelperError> {
    use std::os::windows::{fs::MetadataExt, io::AsRawHandle};
    if file
        .metadata()
        .map_err(|_| HelperError::PermissionDenied)?
        .file_attributes()
        & 0x400
        != 0
    {
        return Err(HelperError::PermissionDenied);
    }
    #[repr(C)]
    struct FileIdInfo {
        volume: u64,
        id: [u8; 16],
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandleEx(
            file: *mut std::ffi::c_void,
            class: u32,
            info: *mut std::ffi::c_void,
            size: u32,
        ) -> i32;
    }
    let mut info = FileIdInfo {
        volume: 0,
        id: [0; 16],
    };
    // SAFETY: held File owns the original valid handle, output has FILE_ID_INFO's
    // exact volume64/id128 ABI. Unsupported providers are never truncated.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle().cast(),
            18,
            (&mut info as *mut FileIdInfo).cast(),
            24,
        )
    } == 0
    {
        return Err(HelperError::UnsupportedIdentity);
    }
    format_windows(info.volume, info.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pure_root_identity_keeps_full_native_widths_and_os_tags() {
        assert_eq!(
            format_linux(u64::MAX, u64::MAX).argument(),
            "root-v1:linux:ffffffffffffffff:ffffffffffffffff"
        );
        assert_eq!(
            format_darwin(u32::MAX, u64::MAX).argument(),
            "root-v1:darwin:ffffffff:ffffffffffffffff"
        );
        let mut id = [0u8; 16];
        id[0] = 0x12;
        id[15] = 0xfe;
        assert_eq!(
            format_windows(u64::MAX, id).unwrap().argument(),
            "root-v1:windows:ffffffffffffffff:120000000000000000000000000000fe"
        );
        assert!(matches!(
            format_windows(0, [0; 16]),
            Err(HelperError::UnsupportedIdentity)
        ));
    }
}
