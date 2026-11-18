use super::AdapterError;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

const MAX_IMAGE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrustedHash([u8; 32]);
impl TrustedHash {
    /// Supply the digest from a trusted build or resource manifest. Hashing an
    /// arbitrary executable found on disk must never establish its own trust.
    pub fn parse(hex: &str) -> Result<Self, AdapterError> {
        if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(AdapterError::InvalidResource);
        }
        let mut bytes = [0u8; 32];
        for (index, pair) in hex.as_bytes().chunks_exact(2).enumerate() {
            let high = digit(pair[0]).ok_or(AdapterError::InvalidResource)?;
            let low = digit(pair[1]).ok_or(AdapterError::InvalidResource)?;
            bytes[index] = (high << 4) | low;
        }
        Ok(Self(bytes))
    }
}
fn digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Keeps a read-only image handle alive. On Windows this denies write/delete
/// sharing while the resource is retained. It is not a hostile same-user
/// filesystem-race sandbox; install resources under the configured trusted root.
pub struct VerifiedHelper {
    path: PathBuf,
    _image: File,
}
impl VerifiedHelper {
    pub fn open(path: &Path, expected: TrustedHash) -> Result<Self, AdapterError> {
        if !path.is_absolute()
            || !path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("exe"))
        {
            return Err(AdapterError::InvalidResource);
        }
        let metadata =
            std::fs::symlink_metadata(path).map_err(|_| AdapterError::InvalidResource)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(AdapterError::InvalidResource);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(AdapterError::InvalidResource);
            }
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            };
            options
                .share_mode(FILE_SHARE_READ)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let mut image = options
            .open(path)
            .map_err(|_| AdapterError::InvalidResource)?;
        let metadata = image
            .metadata()
            .map_err(|_| AdapterError::InvalidResource)?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_IMAGE_BYTES {
            return Err(AdapterError::InvalidResource);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(AdapterError::InvalidResource);
            }
        }
        let mut hash = Sha256::new();
        let mut read_bytes = 0u64;
        let mut buffer = [0u8; 65536];
        loop {
            let count = image
                .read(&mut buffer)
                .map_err(|_| AdapterError::InvalidResource)?;
            if count == 0 {
                break;
            }
            read_bytes += count as u64;
            if read_bytes > metadata.len() || read_bytes > MAX_IMAGE_BYTES {
                return Err(AdapterError::InvalidResource);
            }
            hash.update(&buffer[..count]);
        }
        if read_bytes != metadata.len() {
            return Err(AdapterError::InvalidResource);
        }
        let actual: [u8; 32] = hash.finalize().into();
        if actual != expected.0 {
            return Err(AdapterError::IntegrityMismatch);
        }
        let path = std::fs::canonicalize(path).map_err(|_| AdapterError::InvalidResource)?;
        #[cfg(windows)]
        let path = crate::environment::lookup_path(path);
        Ok(Self {
            path,
            _image: image,
        })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
}
