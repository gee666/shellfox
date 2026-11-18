use crate::{normalize_private_path, HelperError, PermissionVerifier, PrivatePathKind};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
#[path = "state_io.rs"]
mod io;

const MARKER_LIMIT: u64 = 256;
struct StateRoot {
    path: PathBuf,
    _directory: fs::File,
    marker: Mutex<fs::File>,
    fresh: bool,
}

/// A path/provenance capability, never proof of a live or unlocked node.
/// The caller supplies its fixed Rust-owned app path. No installed state is searched.
#[derive(Clone)]
pub struct IsolatedStateDirectory(Arc<StateRoot>);
impl std::fmt::Debug for IsolatedStateDirectory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IsolatedStateDirectory([app-owned])")
    }
}
impl IsolatedStateDirectory {
    pub const MARKER_FILE: &'static str = ".shellfox-rust-state-v1";

    /// Exclusively create a new isolated root, never adopt/overwrite existing data.
    /// Persistent callers use create_new_private so Windows ACLs are proved before
    /// any node starts. This synchronous constructor remains usable by empty fixtures.
    pub fn create_new(path: PathBuf) -> Result<Self, HelperError> {
        let path = explicit_path(path)?;
        let parent = path.parent().ok_or(HelperError::InvalidRequest)?;
        let _parent = io::directory(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&path)
                .map_err(|_| HelperError::Configuration)?;
        }
        #[cfg(not(unix))]
        fs::create_dir(&path).map_err(|_| HelperError::Configuration)?;
        let directory = io::directory(&path)?;
        let path = canonical_path(&path)?;
        let mut marker = io::create_marker(&directory, &path.join(Self::MARKER_FILE))?;
        marker
            .write_all(&expected_marker(&path)?)
            .map_err(|_| HelperError::Configuration)?;
        marker.sync_all().map_err(|_| HelperError::Configuration)?;
        marker
            .seek(SeekFrom::Start(0))
            .map_err(|_| HelperError::Configuration)?;
        Ok(Self(Arc::new(StateRoot {
            path,
            _directory: directory,
            marker: Mutex::new(marker),
            fresh: true,
        })))
    }

    /// Initial persistent creation. Only this newly created root/marker are
    /// protected; an existing parent is checked, never repaired or adopted.
    pub async fn create_new_private(
        path: PathBuf,
        verifier: &PermissionVerifier,
    ) -> Result<Self, HelperError> {
        let path = explicit_path(path)?;
        let parent = path.parent().ok_or(HelperError::InvalidRequest)?;
        let _parent = io::directory(parent)?;
        verifier.check(parent, PrivatePathKind::Directory).await?;
        let root = Self::create_new(path)?;
        root.protect_created(verifier).await?;
        root.verify(verifier).await?;
        Ok(root)
    }

    /// Reopen only the exact explicit Rust-owned path recorded by create_new.
    /// Check-only: no ACL repair, marker migration, fallback, credential reads,
    /// auto-discovery or node startup occurs here. Unmarked production data refuses.
    pub async fn open_existing(
        path: PathBuf,
        verifier: &PermissionVerifier,
    ) -> Result<Self, HelperError> {
        let path = explicit_path(path)?;
        let parent = path.parent().ok_or(HelperError::InvalidRequest)?;
        let _parent = io::directory(parent)?;
        let directory = io::directory(&path)?;
        let canonical = canonical_path(&path)?;
        let marker = io::open_marker(&directory, &canonical.join(Self::MARKER_FILE))?;
        verifier.check(parent, PrivatePathKind::Directory).await?;
        let path = canonical;
        let root = Self(Arc::new(StateRoot {
            path,
            _directory: directory,
            marker: Mutex::new(marker),
            fresh: false,
        }));
        root.verify(verifier).await?;
        Ok(root)
    }
    pub(crate) async fn prepare_for_start(
        &self,
        verifier: &PermissionVerifier,
    ) -> Result<(), HelperError> {
        verifier
            .check(
                self.path().parent().ok_or(HelperError::InvalidRequest)?,
                PrivatePathKind::Directory,
            )
            .await?;
        if self.0.fresh {
            self.protect_created(verifier).await?;
        }
        self.verify(verifier).await
    }
    async fn protect_created(&self, verifier: &PermissionVerifier) -> Result<(), HelperError> {
        verifier
            .protect(self.path(), PrivatePathKind::Directory)
            .await?;
        verifier
            .protect(&self.marker_path(), PrivatePathKind::File)
            .await
    }
    async fn verify(&self, verifier: &PermissionVerifier) -> Result<(), HelperError> {
        verifier
            .check(self.path(), PrivatePathKind::Directory)
            .await?;
        verifier
            .check(&self.marker_path(), PrivatePathKind::File)
            .await?;
        let mut file = self
            .0
            .marker
            .lock()
            .map_err(|_| HelperError::StateMarkerInvalid)?;
        io::validate_marker(&file)?;
        let length = file
            .metadata()
            .map_err(|_| HelperError::StateMarkerInvalid)?
            .len();
        if length > MARKER_LIMIT {
            return Err(HelperError::StateMarkerInvalid);
        }
        file.seek(SeekFrom::Start(0))
            .map_err(|_| HelperError::StateMarkerInvalid)?;
        let mut bytes = Vec::new();
        (&mut *file)
            .take(MARKER_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| HelperError::StateMarkerInvalid)?;
        if bytes != expected_marker(self.path())? {
            return Err(HelperError::StateMarkerInvalid);
        }
        Ok(())
    }
    pub fn path(&self) -> &Path {
        &self.0.path
    }
    fn marker_path(&self) -> PathBuf {
        self.0.path.join(Self::MARKER_FILE)
    }
}
fn explicit_path(path: PathBuf) -> Result<PathBuf, HelperError> {
    let text = path.to_str().ok_or(HelperError::InvalidRequest)?;
    if text.len() > 32767 || text.chars().any(char::is_control) {
        return Err(HelperError::InvalidRequest);
    }
    normalize_private_path(&path)
}
fn canonical_path(path: &Path) -> Result<PathBuf, HelperError> {
    let canonical = fs::canonicalize(path).map_err(|_| HelperError::PermissionDenied)?;
    normalize_private_path(&canonical)
}
fn expected_marker(path: &Path) -> Result<Vec<u8>, HelperError> {
    let path = path.to_str().ok_or(HelperError::InvalidRequest)?;
    Ok(format!(
        "{{\"kind\":\"shellfox-rust-tailscale-state\",\"version\":1,\"pathSha256\":\"{:x}\"}}\n",
        Sha256::digest(path.as_bytes())
    )
    .into_bytes())
}
