//! Existing portable inventory decoding, path bounds and original resource leases.
use super::super::NativeProgramResource;
use crate::system;
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
    sync::Arc,
};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    format: String,
    kind: String,
    platform: String,
    arch: String,
    files: Vec<Entry>,
    binaries: Vec<Binary>,
    #[serde(rename = "version", default)]
    _version: Option<serde::de::IgnoredAny>,
    #[serde(rename = "target", default)]
    _target: Option<serde::de::IgnoredAny>,
    #[serde(rename = "resources", default)]
    _resources: Option<serde::de::IgnoredAny>,
    #[serde(rename = "cargoLockSha256", default)]
    _cargo_lock_sha256: Option<serde::de::IgnoredAny>,
    #[serde(rename = "securityAudit", default)]
    _security_audit: Option<serde::de::IgnoredAny>,
    #[serde(rename = "licenseReviewRequired", default)]
    _license_review_required: Option<serde::de::IgnoredAny>,
    #[serde(rename = "resourceRoot", default)]
    _resource_root: Option<serde::de::IgnoredAny>,
    #[serde(rename = "dataSelection", default)]
    _data_selection: Option<serde::de::IgnoredAny>,
    #[serde(rename = "update", default)]
    _update: Option<serde::de::IgnoredAny>,
    #[serde(rename = "signing", default)]
    _signing: Option<serde::de::IgnoredAny>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Binary {
    package: String,
    path: String,
}
pub(super) struct Inventory {
    pub(super) root: PathBuf,
    manifest: NativeProgramResource,
    files: BTreeMap<String, NativeProgramResource>,
}
impl Inventory {
    pub(super) fn validate_current(&self) -> Result<()> {
        self.manifest.validate_current()?;
        closed_tree(&self.root, &self.files)?;
        for resource in self.files.values() {
            resource.validate_current()?;
        }
        Ok(())
    }
}
fn digest(text: &str) -> Result<[u8; 32]> {
    ensure!(
        text.len() == 64
            && text
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
        "Invalid portable file digest"
    );
    let mut value = [0; 32];
    for (index, byte) in value.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)?;
    }
    Ok(value)
}
fn relative(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty() && value.len() <= 512 && !value.contains('\\'),
        "Invalid portable relative path"
    );
    let parts: Vec<_> = value.split('/').collect();
    ensure!(parts.len() <= 16, "Portable path depth");
    for part in parts {
        ensure!(
            !part.is_empty()
                && part.len() <= 128
                && part != "."
                && part != ".."
                && !part.ends_with('.')
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
            "Invalid portable path component"
        );
        let stem = part
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        ensure!(
            !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                && !(stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.as_bytes()[3].is_ascii_digit()),
            "Ambiguous portable device path"
        );
    }
    Ok(())
}
fn check_tree(root: &Path, name: &str) -> Result<PathBuf> {
    relative(name)?;
    let mut path = root.to_owned();
    for part in name.split('/') {
        path.push(part);
        let metadata = std::fs::symlink_metadata(&path)?;
        ensure!(
            !metadata.file_type().is_symlink(),
            "Portable aliases are not accepted"
        );
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            ensure!(
                metadata.file_attributes() & 0x400 == 0,
                "Portable reparse points are not accepted"
            );
        }
    }
    Ok(path)
}
fn closed_tree(root: &Path, expected: &BTreeMap<String, NativeProgramResource>) -> Result<()> {
    let mut pending = vec![root.to_owned()];
    let mut seen = BTreeSet::new();
    let mut count = 0usize;
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            count += 1;
            ensure!(
                count <= expected.len().saturating_mul(16) + 1,
                "Portable tree entry bound"
            );
            let path = entry.path();
            let name = path
                .strip_prefix(root)?
                .to_str()
                .context("Portable path encoding")?
                .replace('\\', "/");
            if name == "package-manifest.json" {
                continue;
            }
            check_tree(root, &name)?;
            if entry.file_type()?.is_dir() {
                pending.push(path);
            } else {
                ensure!(
                    entry.file_type()?.is_file() && expected.contains_key(&name),
                    "Unmanifested portable file"
                );
                seen.insert(name);
            }
        }
    }
    ensure!(
        seen.len() == expected.len(),
        "Portable inventory is incomplete"
    );
    Ok(())
}

pub(super) fn load_inventory(path: &Path, expected_digest: [u8; 32]) -> Result<Arc<Inventory>> {
    system::validate_system_path(path).map_err(|e| anyhow::anyhow!(e.message))?;
    ensure!(
        !path
            .components()
            .any(|p| matches!(p, Component::ParentDir | Component::CurDir))
            && path
                .file_name()
                .is_some_and(|n| n == "package-manifest.json"),
        "Explicit portable manifest path required"
    );
    let root = path.parent().context("Portable root missing")?;
    ensure!(
        !std::fs::symlink_metadata(root)?.file_type().is_symlink(),
        "Portable root alias refused"
    );
    ensure!(
        std::fs::symlink_metadata(path)?.len() <= 2 * 1024 * 1024,
        "Portable manifest size bound"
    );
    let manifest_resource =
        NativeProgramResource::from_pinned_file(path.to_owned(), expected_digest)?;
    let bytes = manifest_resource.read_verified(2 * 1024 * 1024)?;
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    ensure!(
        manifest.format == "shellfox-rust-portable-v1"
            && matches!(manifest.kind.as_str(), "headless" | "desktop"),
        "Unsupported portable format/kind"
    );
    ensure!(
        cfg!(any(windows, target_os = "linux", target_os = "macos")),
        "Portable host resources are unavailable on this OS"
    );
    let platform = if cfg!(windows) {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "unsupported"
    };
    ensure!(
        manifest.platform == platform && manifest.arch == arch,
        "Portable package target differs from this host"
    );
    ensure!(
        !manifest.files.is_empty() && manifest.files.len() <= 2048,
        "Portable file count bound"
    );
    let mut files = BTreeMap::new();
    let mut folded = BTreeSet::new();
    let mut total = 0u64;
    for entry in manifest.files {
        ensure!(
            entry.path != "package-manifest.json" && folded.insert(entry.path.to_ascii_lowercase()),
            "Duplicate/reserved portable file"
        );
        let file = check_tree(root, &entry.path)?;
        total = total
            .checked_add(std::fs::symlink_metadata(&file)?.len())
            .context("Portable total size overflow")?;
        ensure!(total <= 2 * 1024 * 1024 * 1024, "Portable total size bound");
        let resource = NativeProgramResource::from_pinned_file(file, digest(&entry.sha256)?)?;
        files.insert(entry.path, resource);
    }
    let cli_name = if cfg!(windows) {
        "bin/shellfox-cli.exe"
    } else {
        "bin/shellfox-cli"
    };
    let mut binaries = BTreeSet::new();
    for binary in &manifest.binaries {
        ensure!(
            matches!(binary.package.as_str(), "shellfox-cli" | "shellfox-desktop")
                && binaries.insert(&binary.package)
                && files.contains_key(&binary.path),
            "Invalid portable binary declaration"
        );
    }
    ensure!(
        manifest
            .binaries
            .iter()
            .filter(|b| b.package == "shellfox-cli" && b.path == cli_name)
            .count()
            == 1,
        "Portable CLI declaration missing"
    );
    let cli = files
        .get(cli_name)
        .context("Portable CLI inventory missing")?
        .path()
        .to_owned();
    closed_tree(root, &files)?;
    use system::SystemFiles;
    ensure!(
        system::NativeSystemFiles
            .program(&cli)
            .map_err(|e| anyhow::anyhow!(e.message))?,
        "Portable CLI is not executable"
    );
    let inventory = Arc::new(Inventory {
        root: root.to_owned(),
        manifest: manifest_resource,
        files,
    });
    Ok(inventory)
}
