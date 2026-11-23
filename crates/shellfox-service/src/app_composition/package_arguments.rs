//! Optional trusted portable-package selection; validation does not discover resources.
use super::ApplicationProviderResources;
use anyhow::{Context, Result, bail, ensure};
use std::{
    ffi::OsString,
    path::{Component, Path, PathBuf},
};

#[derive(Default)]
pub(super) struct PackageArguments {
    path: Option<PathBuf>,
    expected_digest: Option<[u8; 32]>,
}
impl PackageArguments {
    pub fn push(&mut self, flag: &str, value: OsString) -> Result<()> {
        match flag {
            "--package-manifest" => {
                ensure!(self.path.is_none(), "Duplicate --package-manifest");
                let path = PathBuf::from(value);
                ensure!(
                    path.is_absolute()
                        && !path.components().any(|part| part == Component::ParentDir),
                    "--package-manifest requires an absolute path without parent traversal"
                );
                self.path = Some(path);
            }
            "--package-manifest-sha256" => {
                ensure!(
                    self.expected_digest.is_none(),
                    "Duplicate --package-manifest-sha256"
                );
                let value = value
                    .to_str()
                    .context("Package digest must be UTF-8 hexadecimal")?;
                ensure!(
                    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()),
                    "--package-manifest-sha256 requires exactly 64 externally trusted hex digits"
                );
                let mut digest = [0; 32];
                for (index, byte) in digest.iter_mut().enumerate() {
                    *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)?;
                }
                self.expected_digest = Some(digest);
            }
            _ => bail!("Unknown package input flag"),
        }
        Ok(())
    }
    pub fn finish(self, data: &Path) -> Result<Option<PackageManifestInput>> {
        match (self.path, self.expected_digest) {
            (None, None) => Ok(None),
            (Some(path), Some(expected_digest)) => {
                let root = path
                    .parent()
                    .context("Package manifest requires a package directory")?;
                ensure!(
                    !super::lexical_overlap(root, data),
                    "Stable package resources must not overlap mutable Host data"
                );
                Ok(Some(PackageManifestInput {
                    path,
                    expected_digest,
                }))
            }
            _ => bail!(
                "--package-manifest and --package-manifest-sha256 must be supplied together; the digest must come from the trusted producer, not a runtime self-hash"
            ),
        }
    }
}
#[derive(Clone, Debug)]
pub(super) struct PackageManifestInput {
    path: PathBuf,
    expected_digest: [u8; 32],
}
impl PackageManifestInput {
    pub fn resources(&self) -> Result<ApplicationProviderResources> {
        ApplicationProviderResources::from_portable_manifest(
            &self.path,
            self.expected_digest,
            crate::system::SystemInstallPolicy {
                // Inventory does not contain the required data/helper launch context.
                allow_user_install: false,
                allow_open: false,
            },
        )
    }
}

#[cfg(test)]
mod tests;
