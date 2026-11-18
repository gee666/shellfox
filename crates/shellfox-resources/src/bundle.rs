use crate::{
    contracts, managed, tree::HeldTree, BundleIdentity, BundleManifest, Error,
    ExpectedManifestDigest, FileSet, ProtocolContract, ProtocolId, Result, TrustedBundleId,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Owns the original manifest, directory and payload handles, not reopened
/// replacements. Windows handles deny write/delete sharing for their lifetime.
pub struct VerifiedBundle {
    manifest: BundleManifest,
    expected_digest: ExpectedManifestDigest,
    tree: HeldTree,
}
impl std::fmt::Debug for VerifiedBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedBundle")
            .field("identity", &self.manifest.identity)
            .finish_non_exhaustive()
    }
}
impl VerifiedBundle {
    pub fn open(
        root: &Path,
        expected_digest: ExpectedManifestDigest,
        expected_identity: &BundleIdentity,
    ) -> Result<Arc<Self>> {
        let (mut tree, bytes) = HeldTree::open(root, expected_digest.as_bytes())?;
        let manifest: BundleManifest = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Manifest("invalid, duplicate or unknown manifest field"))?;
        contracts::validate(&manifest)?;
        if &manifest.identity != expected_identity {
            return Err(Error::Identity);
        }
        tree.acquire_payload(&manifest.files)?;
        managed::validate_runtime(&manifest, &tree)?;
        Ok(Arc::new(Self {
            manifest,
            expected_digest,
            tree,
        }))
    }
    pub fn identity(&self) -> &BundleIdentity {
        &self.manifest.identity
    }
    pub fn file_set(&self) -> &FileSet {
        &self.manifest.files
    }
    pub fn manifest(&self) -> &BundleManifest {
        &self.manifest
    }
    pub fn expected_manifest_digest(&self) -> ExpectedManifestDigest {
        self.expected_digest
    }
    pub fn revalidate(&self) -> Result<()> {
        self.tree.revalidate(&self.manifest.files)
    }
    /// Returns authenticated owned bytes from the original handle. Useful for
    /// WSL script transfer. It does not attest a guest filesystem or interpreter.
    pub fn read_verified(&self, relative: &str, maximum: u64) -> Result<Vec<u8>> {
        self.tree.read_verified(relative, maximum)
    }
    pub fn executable(
        self: &Arc<Self>,
        relative: &str,
        protocol: ProtocolId,
    ) -> Result<VerifiedExecutable> {
        let entry = self
            .manifest
            .entrypoints
            .iter()
            .find(|entry| entry.path == relative)
            .ok_or(Error::Manifest("entrypoint not in trusted manifest"))?;
        let contract = entry
            .protocols
            .iter()
            .find(|contract| contract.id == protocol)
            .ok_or(Error::Manifest("expected protocol not declared"))?
            .clone();
        #[cfg(windows)]
        let path = crate::platform::final_path(
            &self
                .tree
                .files
                .get(relative)
                .ok_or(Error::Manifest("entrypoint handle missing"))?
                .handle,
        )?;
        #[cfg(unix)]
        let path = self.tree.root_path.join(relative);
        Ok(VerifiedExecutable {
            bundle: Arc::clone(self),
            relative: relative.into(),
            path,
            contract,
        })
    }
}

/// Retain this value with the helper process until exit is confirmed. A clone
/// owns the whole bundle, including managed dependencies and lazy-load assets.
#[derive(Clone, Debug)]
pub struct VerifiedExecutable {
    bundle: Arc<VerifiedBundle>,
    relative: String,
    path: PathBuf,
    contract: ProtocolContract,
}
impl VerifiedExecutable {
    pub fn bundle(&self) -> &Arc<VerifiedBundle> {
        &self.bundle
    }
    pub fn relative_path(&self) -> &str {
        &self.relative
    }
    pub fn protocol(&self) -> &ProtocolContract {
        &self.contract
    }
    pub fn revalidate(&self) -> Result<()> {
        self.bundle.revalidate()
    }
    /// No Unix path-spawn promise: read handles there do not prevent in-place
    /// writes or replacement. Descriptor-backed execution needs native proof.
    pub fn path_for_spawn(&self) -> Result<&Path> {
        let path = self.checked_native_path()?;
        if self.bundle.identity().id == TrustedBundleId::WindowsManagedHelper {
            return Err(Error::LoaderPolicyRequired);
        }
        Ok(path)
    }
    pub fn path_for_spawn_with_loader_policy(
        &self,
        policy: &crate::NoDynamicCodeLoad,
    ) -> Result<&Path> {
        policy.verify(self.bundle.expected_manifest_digest())?;
        self.checked_native_path()
    }
    pub(crate) fn checked_native_path(&self) -> Result<&Path> {
        self.revalidate()?;
        if !cfg!(windows)
            || !matches!(
                self.bundle.identity().id,
                TrustedBundleId::WindowsManagedHelper | TrustedBundleId::GoTailscale
            )
        {
            return Err(Error::ImmutableExecutionUnavailable);
        }
        Ok(&self.path)
    }
    pub fn command(&self) -> Result<crate::VerifiedCommand> {
        crate::VerifiedCommand::new(self.clone(), None)
    }
    pub fn command_with_loader_policy(
        &self,
        policy: &crate::NoDynamicCodeLoad,
    ) -> Result<crate::VerifiedCommand> {
        policy.verify(self.bundle.expected_manifest_digest())?;
        crate::VerifiedCommand::new(self.clone(), Some(*policy))
    }
}
