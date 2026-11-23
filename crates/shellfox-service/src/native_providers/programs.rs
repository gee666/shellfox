//! Explicit producer-pinned programs. These inputs are native bootstrap authority,
//! not wire data, package discovery or a runtime-generated trust anchor.
use anyhow::Result;
use shellfox_transport::VerifiedHelper;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone)]
pub struct NativeProgramResource {
    path: PathBuf,
    expected_sha256: [u8; 32],
    // Retains the caller's original validated package/cache lease as well.
    _verified: VerifiedHelper,
    #[cfg(windows)]
    pinned_windows: Option<PinnedWindows>,
}
#[cfg(windows)]
#[derive(Clone)]
struct PinnedWindows {
    original: Arc<std::fs::File>,
    revision: crate::system::FileRevision,
}
#[cfg(windows)]
impl PinnedWindows {
    fn validate(&self, path: &Path) -> Result<()> {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        let revision = crate::system::file_revision(&self.original)
            .map_err(|error| anyhow::anyhow!(error.message))?;
        anyhow::ensure!(
            revision == self.revision,
            "Original pinned program lease changed."
        );
        let metadata = std::fs::symlink_metadata(path)?;
        anyhow::ensure!(
            !metadata.file_type().is_symlink() && metadata.file_attributes() & 0x400 == 0,
            "Pinned program alias rejected."
        );
        let mut options = std::fs::OpenOptions::new();
        options.read(true).share_mode(1).custom_flags(0x00200000);
        let current = options.open(path)?;
        let current = crate::system::file_revision(&current)
            .map_err(|error| anyhow::anyhow!(error.message))?;
        anyhow::ensure!(
            current == self.revision,
            "Configured program path no longer names its original lease."
        );
        Ok(())
    }
}
#[cfg(windows)]
fn local_ntfs(path: &Path, file: &std::fs::File) -> bool {
    use std::{
        os::windows::{ffi::OsStrExt, io::AsRawHandle},
        path::Prefix,
    };
    let drive = match path.components().next() {
        Some(std::path::Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => drive,
            _ => return false,
        },
        _ => return false,
    };
    let root: Vec<u16> = std::ffi::OsStr::new(&format!("{}:\\\0", char::from(drive)))
        .encode_wide()
        .collect();
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetDriveTypeW(root: *const u16) -> u32;
        fn GetVolumeInformationByHandleW(
            file: *mut std::ffi::c_void,
            name: *mut u16,
            name_len: u32,
            serial: *mut u32,
            max_component: *mut u32,
            flags: *mut u32,
            filesystem: *mut u16,
            filesystem_len: u32,
        ) -> i32;
    }
    let mut filesystem = [0u16; 16];
    // Read-only queries on this owned file. Other filesystems/network drives
    // keep hashing rather than treating legacy identity widths as proof.
    if unsafe { GetDriveTypeW(root.as_ptr()) } != 3
        || unsafe {
            GetVolumeInformationByHandleW(
                file.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                filesystem.as_mut_ptr(),
                filesystem.len() as u32,
            )
        } == 0
    {
        return false;
    }
    let end = filesystem
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(filesystem.len());
    String::from_utf16_lossy(&filesystem[..end]).eq_ignore_ascii_case("NTFS")
}
impl NativeProgramResource {
    /// Externally pinned program input for real startup. Own and authenticate the
    /// actual opened file, then retain it as the resource lease. No self-hash.
    pub fn from_pinned_file(path: PathBuf, expected_sha256: [u8; 32]) -> Result<Self> {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1);
        }
        let verified = VerifiedHelper::new(path.clone(), expected_sha256)?;
        let original = options.open(&path)?;
        let original = Arc::new(verified.verify_held_file(original)?);
        #[cfg(windows)]
        let pinned_windows = if local_ntfs(&path, &original) {
            // Identity/revision is only an optimization proof. If unavailable,
            // preserve the existing full digest path instead of claiming a pin.
            crate::system::file_revision(&original)
                .ok()
                .map(|revision| PinnedWindows {
                    original: original.clone(),
                    revision,
                })
        } else {
            None
        };
        let verified = verified.with_resource_guard(original);
        let resource = Self {
            path,
            expected_sha256,
            _verified: verified,
            #[cfg(windows)]
            pinned_windows,
        };
        resource.validate_current()?;
        Ok(resource)
    }
    /// The expected digest and lease must come from an explicitly trusted
    /// producer. Hashing current_exe or an untrusted manifest is not that proof.
    pub fn from_trusted_producer(
        path: PathBuf,
        expected_sha256: [u8; 32],
        lease: Arc<dyn Send + Sync>,
    ) -> Result<Self> {
        let verified =
            VerifiedHelper::new(path.clone(), expected_sha256)?.with_resource_guard(lease);
        Ok(Self {
            path,
            expected_sha256,
            _verified: verified,
            #[cfg(windows)]
            pinned_windows: None,
        })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn read_verified(&self, maximum: usize) -> Result<Vec<u8>> {
        Ok(self._verified.read_verified(maximum)?)
    }
    pub fn validate_current(&self) -> Result<()> {
        #[cfg(windows)]
        if let Some(pinned) = &self.pinned_windows {
            return pinned.validate(&self.path);
        }
        VerifiedHelper::new(self.path.clone(), self.expected_sha256)?;
        Ok(())
    }
}
impl std::fmt::Debug for NativeProgramResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("NativeProgramResource");
        debug.field("path", &self.path);
        #[cfg(windows)]
        debug.field("retained_local_ntfs_lease", &self.pinned_windows.is_some());
        debug.finish_non_exhaustive()
    }
}
