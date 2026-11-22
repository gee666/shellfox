//! `shellfox update` for headless packages. Same flow and JSON as 0.9.x:
//! verify the latest stable release asset, install it beside the anchor, then
//! select it for future launches. Running hosts are never touched.
use crate::{
    headless_layout::{self as layout, arch, platform, root_name},
    headless_package as package,
    output::CliError,
};
use anyhow::{ensure, Context, Result};
use reqwest::{redirect::Policy, Client, Url};
use semver::Version;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

const RELEASES: &str = "https://github.com/gee666/shellfox/releases";
const LATEST: &str = "https://api.github.com/repos/gee666/shellfox/releases/latest";
const AGENT: &str = "shellfox-updater";
const METADATA_LIMIT: usize = 2 * 1024 * 1024;
const ASSET_LIMIT: u64 = 768 * 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateResult {
    pub current: String,
    pub latest: Option<String>,
    pub available: bool,
    pub phase: &'static str,
    pub installation: Option<PathBuf>,
    pub restart_required: bool,
    pub message: String,
}

pub async fn run(check: bool) -> Result<UpdateResult> {
    let current = env!("SHELLFOX_VERSION");
    let root = layout::current_root().ok_or_else(|| {
        CliError::new("UNSUPPORTED", "Headless updates require an extracted headless release package. Desktop installs use their own updater.", false)
    })?;
    package::validate(&root, current, platform(), arch())?;
    let release = latest().await.map_err(|cause| {
        CliError::new("UPDATE_FAILED", format!("Could not check the latest Shellfox release. {cause:#} Check your connection or see {RELEASES}"), true)
    })?;
    let Some((version, asset)) = release else {
        return Ok(result(
            current,
            None,
            "current",
            "No published Shellfox release yet.",
        ));
    };
    let available = version > Version::parse(current)?;
    let latest = version.to_string();
    if !available {
        return Ok(result(
            current,
            Some(latest),
            "current",
            &format!("Shellfox is up to date ({current})."),
        ));
    }
    let asset = asset.ok_or_else(|| {
        CliError::new("UNSUPPORTED", format!("Release {latest} has no unique, SHA-256-verified headless archive for {}/{}. See {RELEASES}", platform(), arch()), false)
    })?;
    let mut found = result(current, Some(latest.clone()), "available", &format!("Shellfox {latest} is available. Run shellfox update to install it for future launches."));
    if check {
        return Ok(found);
    }
    let anchor = layout::anchor(&root)?;
    let installation = install(&root, &anchor, &latest, |archive| download(&asset, archive))
        .await
        .map_err(|cause| CliError::new("UPDATE_FAILED", format!("Could not install the headless update. {cause:#} The active version and running hosts were not changed. See {RELEASES}"), true))?;
    found.phase = "installed";
    found.restart_required = true;
    found.message = format!(
        "Shellfox {latest} installed for future launches. Keep using the original launcher and service paths at {}. The running host and its terminals are unchanged. Restart the host through its startup manager, or run host stop --yes and host run with the same --user-data. Keep both {} and {}.",
        anchor.display(), anchor.display(), installation.display()
    );
    found.installation = Some(installation);
    Ok(found)
}

fn result(
    current: &str,
    latest: Option<String>,
    phase: &'static str,
    message: &str,
) -> UpdateResult {
    UpdateResult {
        current: current.into(),
        available: phase == "available",
        latest,
        phase,
        installation: None,
        restart_required: false,
        message: message.into(),
    }
}

/// Stage, verify and select one version. Failures leave the pointer unchanged.
pub async fn install<F, Fut>(root: &Path, anchor: &Path, version: &str, fetch: F) -> Result<PathBuf>
where
    F: FnOnce(PathBuf) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let lock = anchor.join("headless-update.lock");
    layout::write_private(&lock, b"", false).map_err(|_| {
        anyhow::anyhow!("Headless update is locked or this installation is not writable. If no update is running, remove {} and retry.", lock.display())
    })?;
    let staged = async {
        ensure!(
            layout::active(root)?.is_none(),
            "Another update was activated during this check. Run shellfox update again."
        );
        let parent = anchor
            .parent()
            .context("Installation has no parent directory")?;
        let work = tempfile::Builder::new()
            .prefix(&format!(
                "shellfox-headless-{version}-{}-{}-",
                platform(),
                arch()
            ))
            .rand_bytes(6)
            .tempdir_in(parent)?;
        let archive = work.path().join(package::asset_name(version));
        fetch(archive.clone()).await?;
        let destination = work.path().join("installation");
        std::fs::create_dir(&destination)?;
        let installation = package::extract(&archive, &destination, &root_name())?;
        package::validate(&installation, version, platform(), arch())?;
        std::fs::remove_file(&archive)?;
        let installation = layout::normalize(&installation);
        layout::activate(anchor, &installation)?;
        let _ = work.keep();
        Ok(installation)
    }
    .await;
    let _ = std::fs::remove_file(&lock);
    staged
}

struct Asset {
    url: Url,
    size: u64,
    sha256: String,
}

async fn latest() -> Result<Option<(Version, Option<Asset>)>> {
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect_policy(Policy::none())
        .build()?;
    let mut response = client
        .get(LATEST)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", AGENT)
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    ensure!(
        response.status().is_success(),
        "Release lookup failed with HTTP {}.",
        response.status()
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= METADATA_LIMIT,
            "Release metadata is too large."
        );
        bytes.extend(chunk);
    }
    let release: serde_json::Value = serde_json::from_slice(&bytes)?;
    let tag = release["tag_name"]
        .as_str()
        .context("Invalid stable release metadata.")?;
    let version = Version::parse(tag.strip_prefix('v').unwrap_or(tag))?;
    ensure!(
        tag.len() <= 65
            && release["draft"] != true
            && release["prerelease"] != true
            && version.pre.is_empty()
            && version.build.is_empty(),
        "Invalid stable release metadata."
    );
    let name = package::asset_name(&version.to_string());
    let matches: Vec<_> = release["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|asset| asset["name"].as_str() == Some(&name))
        .collect();
    let asset = match matches.as_slice() {
        [asset] => parse_asset(asset, tag, &name),
        _ => None,
    };
    Ok(Some((version, asset)))
}

fn parse_asset(asset: &serde_json::Value, tag: &str, name: &str) -> Option<Asset> {
    let mut expected = Url::parse(RELEASES).ok()?;
    expected
        .path_segments_mut()
        .ok()?
        .extend(["download", tag, name]);
    let url: Url = asset["browser_download_url"].as_str()?.parse().ok()?;
    let size = asset["size"].as_u64()?;
    let digest = asset["digest"].as_str()?.strip_prefix("sha256:")?;
    (url == expected
        && (1..=ASSET_LIMIT).contains(&size)
        && digest.len() == 64
        && digest.bytes().all(|b| b.is_ascii_hexdigit()))
    .then(|| Asset {
        url,
        size,
        sha256: digest.to_ascii_lowercase(),
    })
}

fn trusted(url: &Url) -> bool {
    url.username().is_empty()
        && url.password().is_none()
        && url.scheme() == "https"
        && url.port().is_none()
        && matches!(
            url.host_str(),
            Some(
                "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
            )
        )
}

async fn download(asset: &Asset, destination: PathBuf) -> Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(1800))
        .connect_timeout(Duration::from_secs(10))
        .redirect_policy(Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 || !trusted(attempt.url()) {
                attempt.error("Untrusted update redirect")
            } else {
                attempt.follow()
            }
        }))
        .build()?;
    let mut response = client
        .get(asset.url.clone())
        .header("User-Agent", AGENT)
        .send()
        .await?;
    ensure!(
        trusted(response.url()) && response.status().is_success(),
        "Release download failed."
    );
    let mut file = std::fs::File::create_new(&destination)?;
    let (mut hash, mut received) = (Sha256::new(), 0u64);
    while let Some(chunk) = response.chunk().await? {
        received += chunk.len() as u64;
        ensure!(received <= asset.size, "Download exceeds the release size.");
        hash.update(&chunk);
        file.write_all(&chunk)?;
    }
    ensure!(
        received == asset.size && format!("{:x}", hash.finalize()) == asset.sha256,
        "Release checksum verification failed."
    );
    Ok(file.sync_all()?)
}

#[cfg(test)]
#[path = "headless_update_tests.rs"]
mod tests;
