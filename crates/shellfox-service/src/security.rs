use anyhow::{bail, Context, Result};
use shellfox_transport::{BoundOwnershipRoot, PermissionVerifier, PrivatePathKind, VerifiedHelper};
use std::path::{Path, PathBuf};
mod host_path;
pub(crate) use host_path::HostRoot;
mod creation_retention;
pub(crate) use creation_retention::NativeCreationRetention;
#[cfg(test)]
mod cold_tests;
#[cfg(test)]
mod test_support;

pub(crate) const ENDPOINT_DIRECTORY: &str = "rust-service-v1";
/// Terminal upload inbox. Every child the host creates in its root must be
/// listed in `plan`, or the next start refuses the root.
pub(crate) const UPLOADS_DIRECTORY: &str = "uploads";
pub(crate) struct PrivateDirectory {
    pub path: PathBuf,
    pub helper: VerifiedHelper,
    pub ssh_store: crate::local::ssh_read::SshStoreLocation,
    root: HostRoot,
}
impl PrivateDirectory {
    /// Pure original-path inspection. No helper, creation or permission mutation.
    pub fn plan(data: &Path, helper: &Path, digest: [u8; 32], owner: bool) -> Result<Self> {
        let root = HostRoot::inspect(data)?;
        if !owner && !root.exists() {
            bail!("No existing host root is available to connect.");
        }
        let data = root.path().to_owned();
        if root.exists() {
            let entries = std::fs::read_dir(&data).with_context(|| {
                format!(
                    "Cannot inspect the Shellfox data directory: {}",
                    data.display()
                )
            })?;
            for entry in entries {
                let entry = entry?;
                let name = entry.file_name();
                if !matches!(
                    name.to_str(),
                    // terminal-home: left by earlier builds until Service::open_owned removes it.
                    Some(
                        ENDPOINT_DIRECTORY
                            | UPLOADS_DIRECTORY
                            | "terminal-home"
                            | ".host-lock"
                            | "shellfox-rust-v1.lock"
                            | "shellfox-rust-v1.sqlite3"
                            | "shellfox-rust-v1.sqlite3-wal"
                            | "shellfox-rust-v1.sqlite3-shm"
                            | "desktop-window.json"
                            | "keep-awake-lid.txt"
                    )
                ) {
                    bail!("Data directory contains files Shellfox did not create; no permissions were changed.");
                }
                reject_links(
                    &entry.path(),
                    matches!(
                        name.to_str(),
                        Some(
                            ENDPOINT_DIRECTORY | UPLOADS_DIRECTORY | "terminal-home" | ".host-lock"
                        )
                    ),
                )?;
            }
        }
        root.revalidate()?;
        #[cfg(unix)]
        if owner {
            root.validate_creation_ancestors()?;
        }
        // Fresh verification has no earlier resource guard to replace. Every
        // original preparation child also retains this same filesystem owner.
        let helper_path = helper.to_owned();
        let helper = VerifiedHelper::new(helper_path.clone(), digest)?
            .with_resource_guard(root.creation_owner());
        Ok(Self {
            path: data.join(ENDPOINT_DIRECTORY),
            ssh_store: crate::local::ssh_read::SshStoreLocation {
                endpoint_directory: data.join(ENDPOINT_DIRECTORY),
                helper: (helper_path, digest),
            },
            helper,
            root,
        })
    }
    pub async fn prepare_root(
        &mut self,
        preparation: &mut shellfox_transport::OfflinePreparation,
        create: bool,
    ) -> Result<()> {
        let original_count = self.root.original_directory_count();
        // This is before ANY anchor permission helper, including the unbound
        // Windows case. Existing controllers share this exact append-only guard.
        preparation.retain_original_directories(self.root.original_directories_from(0)?)?;
        if !self.root.exists() {
            anyhow::ensure!(create, "Endpoint clients cannot create a host root");
            // The shared primitive verifies the ORIGINAL held parent's trust
            // and supplies private security in the relative creation syscall.
            let created = self.root.create_missing();
            // Even partial creation retains its actual original handles before
            // a further helper, binding, or cancellation point can occur.
            let partial = self.root.original_directories_from(original_count)?;
            if !partial.is_empty() {
                preparation.retain_original_directories(partial)?;
            }
            created?;
        } else if create {
            self.root.repair_existing().with_context(|| {
                format!(
                    "Cannot make the Shellfox data directory private: {}",
                    self.root.path().display()
                )
            })?;
        }
        // Clients never repair permissions. Verify the original directory before binding.
        self.check(
            self.root.path(),
            true,
            false,
            preparation.permission_verifier(),
        )
        .await
    }
    pub fn creation_owner(&self) -> std::sync::Arc<NativeCreationRetention> {
        self.root.creation_owner()
    }
    #[cfg(unix)]
    pub fn endpoint_directory(&self) -> Result<std::fs::File> {
        self.root.private_child_file(ENDPOINT_DIRECTORY)
    }
    pub fn bound_root(&self) -> Result<BoundOwnershipRoot> {
        self.root.bound_root()
    }
    /// Scope over the endpoint directory, bound to the handle held since preparation.
    pub fn bind_native_control(
        &self,
        fence: std::sync::Arc<dyn shellfox_files::NativeCommitFence>,
    ) -> Result<std::sync::Arc<shellfox_files::FileService>> {
        self.root.bind_native_control(fence, ENDPOINT_DIRECTORY)
    }
    pub async fn prepare_owned_layout(&self, verifier: &PermissionVerifier) -> Result<()> {
        self.root.create_child(ENDPOINT_DIRECTORY)?;
        self.check(
            &self.root.path().join(ENDPOINT_DIRECTORY),
            true,
            false,
            verifier,
        )
        .await?;
        self.protect_artifacts(verifier).await
    }
    pub async fn protect_artifacts(&self, verifier: &PermissionVerifier) -> Result<()> {
        let data = self.root.path();
        for entry in std::fs::read_dir(data)? {
            let entry = entry?;
            let name = entry.file_name();
            let directory = matches!(
                name.to_str(),
                Some("rust-service-v1" | "terminal-home" | ".host-lock")
            );
            // Directories were created private or verified by the shared
            // primitive. Existing directories and Go's lock are never repaired.
            self.check(&entry.path(), directory, !directory, verifier)
                .await
                .with_context(|| {
                    format!("Unsafe private data artifact: {}", name.to_string_lossy())
                })?;
        }
        Ok(())
    }
    pub async fn check(
        &self,
        path: &Path,
        directory: bool,
        protect: bool,
        verifier: &PermissionVerifier,
    ) -> Result<()> {
        self.root.revalidate()?;
        reject_links(path, directory)?;
        let kind = if directory {
            PrivatePathKind::Directory
        } else {
            PrivatePathKind::File
        };
        if protect {
            #[cfg(unix)]
            self.root.protect(path, directory)?;
            #[cfg(not(unix))]
            verifier.protect(path, kind).await?;
        }
        verifier.check(path, kind).await?;
        self.root.revalidate()?;
        Ok(())
    }
    pub async fn read_endpoint(&self, _verifier: &PermissionVerifier) -> Result<Vec<u8>> {
        Ok(self
            .root
            .read_private_child_leaf(
                ENDPOINT_DIRECTORY,
                std::ffi::OsStr::new("endpoint.json"),
                4096,
            )?
            .bytes)
    }
    pub async fn read_existing(
        data: &Path,
        helper: &Path,
        digest: [u8; 32],
    ) -> Result<(Self, Vec<u8>)> {
        let mut private = Self::plan(data, helper, digest, false)?;
        let mut preparation = shellfox_transport::OfflinePreparation::new(private.helper.clone())?;
        let result = async {
            private.prepare_root(&mut preparation, false).await?;
            private
                .read_endpoint(preparation.permission_verifier())
                .await
        }
        .await;
        // Never return while a permission helper child may still be running. A slow
        // close (a stalled disk, a frozen helper) is retried and logged every 30 s,
        // never a reason to exit.
        let mut attempts = 0u64;
        loop {
            match preparation.shutdown_confirmed().await {
                Ok(_proof) => break,
                Err(error) => {
                    attempts += 1;
                    if attempts.is_multiple_of(300) {
                        eprintln!(
                            "Shellfox: permission helper close not confirmed after {} s: {error:#}. Still retrying.",
                            attempts / 10
                        );
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await
                }
            }
        }
        Ok((private, result?))
    }
}
pub(crate) fn reject_links(path: &Path, directory: bool) -> Result<()> {
    let info = std::fs::symlink_metadata(path)?;
    if info.file_type().is_symlink()
        || if directory {
            !info.is_dir()
        } else {
            !info.is_file()
        }
    {
        bail!("Unsafe private endpoint path.");
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if info.file_attributes() & 0x400 != 0 {
            bail!("Reparse points are not private endpoint paths.");
        }
    }
    Ok(())
}
