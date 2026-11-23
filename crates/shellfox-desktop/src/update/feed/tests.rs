use super::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

async fn fixture(
    payload: &'static str,
    size: u64,
    hash: &str,
    duplicate: bool,
) -> (Feed, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let asset = serde_json::json!({
        "name": "ShellfoxSetup.exe", "size": size, "digest": format!("sha256:{hash}"),
        "browser_download_url": format!("{origin}/releases/download/v2.0.0/ShellfoxSetup.exe"),
    });
    let json = serde_json::json!({ "tag_name": "v2.0.0", "assets": vec![asset; if duplicate { 2 } else { 1 }] }).to_string();
    let task = tokio::spawn(async move {
        for response in [json.as_str(), payload] {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(stream.read_u8().await.unwrap());
                assert!(request.len() < 8192);
            }
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response.len()
            );
            stream.write_all(header.as_bytes()).await.unwrap();
            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });
    let feed = Feed::with_urls(
        format!("{origin}/latest").parse().unwrap(),
        format!("{origin}/releases").parse().unwrap(),
    )
    .unwrap();
    (feed, task)
}

#[test]
fn product_version_is_newer_than_the_installed_electron_release() {
    let version = Version::parse(env!("SHELLFOX_VERSION")).unwrap();
    assert!(version > Version::new(0, 9, 15));
}

#[tokio::test]
async fn verifies_size_and_digest_from_loopback_feed() {
    let scratch = tempfile::tempdir().unwrap();
    let digest = format!("{:x}", Sha256::digest(b"test installer"));
    for (case, payload, size, hash, valid) in [
        ("valid", "test installer", 14, digest.clone(), true),
        ("short", "test installer", 15, digest.clone(), false),
        ("long", "test installer", 13, digest, false),
        ("digest", "test installer", 14, "00".repeat(32), false),
    ] {
        let (feed, task) = fixture(payload, size, &hash, false).await;
        let release = feed.check(|_| "ShellfoxSetup.exe".into()).await.unwrap();
        assert_eq!(release.version, Version::new(2, 0, 0));
        let file = scratch.path().join(case);
        assert_eq!(
            feed.download(&release.asset.unwrap(), &file, |_| {})
                .await
                .is_ok(),
            valid,
            "{case}"
        );
        if valid {
            assert_eq!(std::fs::read(file).unwrap(), payload.as_bytes());
        }
        task.await.unwrap();
    }
}

#[tokio::test]
async fn duplicate_asset_is_notice_only() {
    let (feed, task) = fixture("unused", 1, &"00".repeat(32), true).await;
    let release = feed.check(|_| "ShellfoxSetup.exe".into()).await.unwrap();
    assert_eq!(release.version, Version::new(2, 0, 0));
    assert!(release.asset.is_none());
    task.abort();
}

#[test]
fn only_trusted_https_origins_can_receive_download_redirects() {
    let source = Url::parse(RELEASES).unwrap();
    for url in [
        "http://github.com/path",
        "https://other.example/",
        "https://release-assets.githubusercontent.com:123/",
        "https://user@github.com/path",
    ] {
        assert!(
            !allowed_asset_origin(&url.parse().unwrap(), &source),
            "{url}"
        );
    }
    assert!(allowed_asset_origin(
        &"https://release-assets.githubusercontent.com/path"
            .parse()
            .unwrap(),
        &source
    ));
}

/// Test builds are published as GitHub pre-releases; installed apps must never pick them up.
#[tokio::test]
async fn prerelease_metadata_is_never_an_update() {
    for (tag, prerelease) in [
        ("v2.0.0", true),
        ("v2.0.0-beta.1", false),
        ("v2.0.0-beta.1", true),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let json = serde_json::json!({ "tag_name": tag, "prerelease": prerelease, "assets": [] })
            .to_string();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(stream.read_u8().await.unwrap());
            }
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                json.len()
            );
            stream.write_all(header.as_bytes()).await.unwrap();
            stream.write_all(json.as_bytes()).await.unwrap();
        });
        let feed = Feed::with_urls(
            format!("{origin}/latest").parse().unwrap(),
            format!("{origin}/releases").parse().unwrap(),
        )
        .unwrap();
        assert!(
            feed.check(|_| "ShellfoxSetup.exe".into()).await.is_err(),
            "{tag} {prerelease}"
        );
        task.await.unwrap();
    }
}

#[test]
fn stable_release_is_newer_than_its_own_prerelease() {
    let stable = Version::new(0, 10, 0);
    assert!(stable > Version::parse("0.10.0-beta.3").unwrap());
    assert!(Version::parse("0.10.1").unwrap() > Version::parse("0.10.1-rc.1").unwrap());
}
