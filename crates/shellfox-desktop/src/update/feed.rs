use anyhow::{Context, Result, ensure};
use reqwest::{Client, Response, Url, redirect::Policy};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs::File, io::Write, path::Path, time::Duration};

pub(crate) const RELEASES: &str = "https://github.com/gee666/shellfox/releases";
const LATEST: &str = "https://api.github.com/repos/gee666/shellfox/releases/latest";
const METADATA_LIMIT: usize = 2 * 1024 * 1024;
const ASSET_LIMIT: u64 = 768 * 1024 * 1024;
const AGENT: &str = "shellfox-updater";

#[derive(Clone)]
pub(super) struct Release {
    pub version: Version,
    pub asset: Option<Asset>,
}
#[derive(Clone)]
pub(super) struct Asset {
    pub name: String,
    url: Url,
    pub size: u64,
    pub sha256: String,
}
#[derive(Deserialize)]
struct Metadata {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<serde_json::Value>,
}

#[derive(Clone)]
pub(super) struct Feed {
    latest: Url,
    release_origin: Url,
    lookup: Client,
    download: Client,
}
impl Feed {
    pub fn new() -> Result<Self> {
        Self::with_urls(Url::parse(LATEST)?, Url::parse(RELEASES)?)
    }
    fn with_urls(latest: Url, release_origin: Url) -> Result<Self> {
        let allowed = release_origin.clone();
        let builder = || {
            let client = Client::builder();
            if latest.host_str() == Some("127.0.0.1") {
                client.no_proxy()
            } else {
                client
            }
        };
        let lookup = builder()
            .timeout(Duration::from_secs(10))
            .redirect_policy(Policy::none())
            .build()?;
        let download = builder()
            .timeout(Duration::from_secs(1800))
            .connect_timeout(Duration::from_secs(10))
            .redirect_policy(Policy::custom(move |attempt| {
                if attempt.previous().len() >= 10 || !allowed_asset_origin(attempt.url(), &allowed)
                {
                    attempt.error("Untrusted update redirect")
                } else {
                    attempt.follow()
                }
            }))
            .build()?;
        Ok(Self {
            latest,
            release_origin,
            lookup,
            download,
        })
    }
    pub async fn check(&self, asset_name: impl FnOnce(&Version) -> String) -> Result<Release> {
        let mut response = self
            .lookup
            .get(self.latest.clone())
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", AGENT)
            .header("Cache-Control", "no-cache")
            .send()
            .await?;
        ensure!(response.status().is_success(), "Release check failed");
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            ensure!(
                bytes.len() + chunk.len() <= METADATA_LIMIT,
                "Release metadata is too large"
            );
            bytes.extend(chunk);
        }
        let release: Metadata = serde_json::from_slice(&bytes)?;
        let tag = release.tag_name.trim();
        let version: Version = tag.strip_prefix('v').unwrap_or(tag).parse()?;
        ensure!(
            tag.len() <= 65
                && !release.draft
                && !release.prerelease
                && version.pre.is_empty()
                && version.build.is_empty(),
            "Not a published stable release"
        );
        let name = asset_name(&version);
        let mut matches = release
            .assets
            .iter()
            .filter(|a| a.get("name").and_then(serde_json::Value::as_str) == Some(&name));
        let asset = matches.next().and_then(|a| {
            if matches.next().is_some() {
                return None;
            }
            let mut expected = self.release_origin.clone();
            expected
                .path_segments_mut()
                .ok()?
                .extend(["download", &release.tag_name, &name]);
            let url: Url = a.get("browser_download_url")?.as_str()?.parse().ok()?;
            let size = a.get("size")?.as_u64()?;
            let digest = a.get("digest")?.as_str()?.strip_prefix("sha256:")?;
            if url != expected
                || !credentials_absent(&url)
                || !(1..=ASSET_LIMIT).contains(&size)
                || digest.len() != 64
                || !digest.bytes().all(|c| c.is_ascii_hexdigit())
            {
                return None;
            }
            Some(Asset {
                name: name.clone(),
                url,
                size,
                sha256: digest.to_ascii_lowercase(),
            })
        });
        Ok(Release { version, asset })
    }
    pub async fn download(
        &self,
        asset: &Asset,
        destination: &Path,
        report: impl Fn(u64),
    ) -> Result<()> {
        let response = self
            .download
            .get(asset.url.clone())
            .header("User-Agent", AGENT)
            .send()
            .await?;
        ensure!(
            allowed_asset_origin(response.url(), &self.release_origin),
            "Untrusted update asset origin"
        );
        ensure!(response.status().is_success(), "Release download failed");
        save(response, asset, destination, report).await
    }
}
fn credentials_absent(url: &Url) -> bool {
    url.username().is_empty() && url.password().is_none()
}
fn allowed_asset_origin(url: &Url, initial: &Url) -> bool {
    credentials_absent(url)
        && (url.origin() == initial.origin()
            || (initial.scheme() == "https"
                && url.scheme() == "https"
                && url.port().is_none()
                && matches!(
                    url.host_str(),
                    Some("release-assets.githubusercontent.com" | "objects.githubusercontent.com")
                )))
}
async fn save(
    mut response: Response,
    asset: &Asset,
    destination: &Path,
    report: impl Fn(u64),
) -> Result<()> {
    let mut file = File::create_new(destination)?;
    let mut hash = Sha256::new();
    let mut received = 0;
    while let Some(chunk) = response.chunk().await? {
        received += chunk.len() as u64;
        ensure!(received <= asset.size, "Download exceeds the release size");
        hash.update(&chunk);
        file.write_all(&chunk)?;
        report(received);
    }
    ensure!(
        received == asset.size && format!("{:x}", hash.finalize()) == asset.sha256,
        "Release checksum verification failed"
    );
    file.sync_all().context("Could not save update")
}

#[cfg(test)]
#[path = "feed_tests.rs"]
mod tests;
