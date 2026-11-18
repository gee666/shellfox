use anyhow::{ensure, Result};
use std::fs::{DirBuilder, File};
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub(super) struct PrivateDirectory {
    path: PathBuf,
    device: u64,
    inode: u64,
}
impl PrivateDirectory {
    pub fn create(base: &Path) -> Result<Self> {
        ensure!(base.is_absolute(), "runtime directory must be absolute");
        let base = base.canonicalize()?;
        for _ in 0..16 {
            let path = base.join(format!("sfx-{}", &random_token()?[..32]));
            match DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => {
                    let value = std::fs::symlink_metadata(&path)?;
                    // SAFETY: getuid has no preconditions.
                    ensure!(
                        value.is_dir()
                            && value.uid() == unsafe { libc::getuid() }
                            && value.permissions().mode() & 0o777 == 0o700,
                        "private control directory ownership failed"
                    );
                    let result = Self {
                        path,
                        device: value.dev(),
                        inode: value.ino(),
                    };
                    ensure!(
                        result.socket().as_os_str().as_encoded_bytes().len() < 104,
                        "private control socket path is too long"
                    );
                    return Ok(result);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        anyhow::bail!("could not create unique private control directory")
    }
    pub fn socket(&self) -> PathBuf {
        self.path.join("s")
    }
    pub fn unchanged(&self) -> Result<()> {
        let value = std::fs::symlink_metadata(&self.path)?;
        ensure!(
            value.is_dir()
                && value.dev() == self.device
                && value.ino() == self.inode
                && value.uid() == unsafe { libc::getuid() }
                && value.mode() & 0o777 == 0o700,
            "private control directory changed"
        );
        Ok(())
    }
}
impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        // Never recursively delete an occupied/replaced directory. The native
        // supervisor unlinks its own authenticated socket after verified close.
        if self.unchanged().is_ok() {
            let _ = std::fs::remove_dir(&self.path);
        }
    }
}
pub(super) fn random_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Capability preflight supplements these checks. Packaging must also verify
/// the configured artifact checksum. Never resolve a helper through PATH.
pub(super) fn helper_path(path: &Path) -> Result<PathBuf> {
    ensure!(path.is_absolute(), "native helper path must be absolute");
    let canonical = path.canonicalize()?;
    let metadata = canonical.metadata()?;
    ensure!(
        metadata.is_file() && metadata.mode() & 0o111 != 0 && metadata.mode() & 0o022 == 0,
        "native helper must be executable and not group/world writable"
    );
    ensure!(
        metadata.uid() == 0 || metadata.uid() == unsafe { libc::getuid() },
        "native helper has unexpected owner"
    );
    Ok(canonical)
}
