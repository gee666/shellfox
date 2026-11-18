use super::{config::guest_path, WslError, WslProfileId};
use std::path::{Component, Path, PathBuf};

/// An explicitly approved staging-root mapping, not a drive-letter guess.
/// The host still authorizes generation/control epoch and commits the upload.
#[derive(Debug, Clone)]
pub struct WslUploadMapping {
    distro: WslProfileId,
    native_root: PathBuf,
    guest_root: String,
}
impl WslUploadMapping {
    pub fn new(
        distro: WslProfileId,
        native_root: PathBuf,
        approved_guest_root: String,
    ) -> Result<Self, WslError> {
        guest_path(&approved_guest_root)?;
        if !native_root.is_absolute() || !native_root.is_dir() {
            return Err(WslError::InvalidRequest);
        }
        crate::validate_isolated_path(
            &native_root,
            &native_root,
            crate::IsolationPathKind::Directory,
        )
        .map_err(|_| WslError::InvalidRequest)?;
        let native_root = native_root
            .canonicalize()
            .map_err(|_| WslError::InvalidRequest)?;
        Ok(Self {
            distro,
            native_root,
            guest_root: approved_guest_root,
        })
    }
    pub fn resolve(&self, distro: &WslProfileId, native: &Path) -> Result<String, WslError> {
        if distro != &self.distro || !native.is_absolute() || !native.is_file() {
            return Err(WslError::InvalidRequest);
        }
        crate::validate_isolated_path(native, &self.native_root, crate::IsolationPathKind::File)
            .map_err(|_| WslError::InvalidRequest)?;
        let canonical = native
            .canonicalize()
            .map_err(|_| WslError::InvalidRequest)?;
        let relative = canonical
            .strip_prefix(&self.native_root)
            .map_err(|_| WslError::InvalidRequest)?;
        let mut guest = self.guest_root.clone();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(WslError::InvalidRequest);
            };
            let name = name.to_str().ok_or(WslError::InvalidRequest)?;
            if name.is_empty()
                || name
                    .chars()
                    .any(|c| c.is_control() || c == '/' || c == '\\')
            {
                return Err(WslError::InvalidRequest);
            }
            if !guest.ends_with('/') {
                guest.push('/');
            }
            guest.push_str(name);
        }
        guest_path(&guest)?;
        Ok(guest)
    }
    pub fn distro(&self) -> &WslProfileId {
        &self.distro
    }
}
