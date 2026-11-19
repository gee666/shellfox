//! Native helper for tests. Never use installed binaries or state.
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

pub fn digest_file(path: &Path) -> Result<[u8; 32]> {
    let mut file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hash = Sha256::new();
    std::io::copy(&mut file, &mut hash)?;
    Ok(hash.finalize().into())
}

pub fn helper() -> Result<(PathBuf, [u8; 32])> {
    static HELPER: OnceLock<Result<(PathBuf, [u8; 32]), String>> = OnceLock::new();
    HELPER
        .get_or_init(|| prepare().map_err(|error| format!("{error:#}")))
        .as_ref()
        .cloned()
        .map_err(|error| anyhow::anyhow!("{error}"))
}

fn prepare() -> Result<(PathBuf, [u8; 32])> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .context("workspace root")?;
    let dir = workspace.join("tmp/dev-helper");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!(
        "shellfox-tailscale{}",
        std::env::consts::EXE_SUFFIX
    ));
    if !path.is_file() {
        // Separate integration test binaries can start at the same time.
        let lock = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join("cargo-test-build.lock"))?;
        lock.lock()?;
        if !path.is_file() {
            let status = Command::new("node")
                .arg(workspace.join("scripts/tailscale.mjs"))
                .arg("build")
                .status()
                .context("run the development helper build")?;
            anyhow::ensure!(
                status.success(),
                "development helper build failed: {status}"
            );
        }
    }
    let digest = digest_file(&path)?;
    Ok((path, digest))
}
