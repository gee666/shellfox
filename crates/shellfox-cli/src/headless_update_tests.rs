use super::*;
use crate::headless_layout::{active, normalize, POINTER};

fn archive(source_parent: &Path, output: &Path) {
    let tar = if cfg!(windows) {
        Path::new(&std::env::var_os("SystemRoot").unwrap()).join("System32/tar.exe")
    } else {
        "/usr/bin/tar".into()
    };
    let mut command = std::process::Command::new(tar);
    if cfg!(windows) {
        command.args(["-a", "-cf"]);
    } else {
        command.arg("-czf");
    }
    let status = command
        .arg(output)
        .arg("-C")
        .arg(source_parent)
        .arg(root_name())
        .status()
        .unwrap();
    assert!(status.success());
}

#[tokio::test(flavor = "current_thread")]
async fn a_verified_archive_is_installed_beside_the_anchor_and_selected() {
    let temp = tempfile::tempdir().unwrap();
    let parent = normalize(temp.path());
    let anchor = parent.join(root_name());
    crate::headless_package::tests::fake_package(&anchor, "0.10.0");
    let source = parent.join("source");
    crate::headless_package::tests::fake_package(&source.join(root_name()), "0.10.1");
    let built = parent.join("built-archive");
    archive(&source, &built);

    let fetch = |destination: PathBuf| {
        let built = built.clone();
        async move { Ok(std::fs::copy(&built, destination).map(|_| ())?) }
    };
    let installation = install(&anchor, &anchor, "0.10.1", fetch).await.unwrap();
    assert_eq!(active(&anchor).unwrap(), Some(installation.clone()));
    assert_eq!(
        installation
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap(),
        parent
    );
    assert!(!anchor.join("headless-update.lock").exists());
    assert!(
        std::fs::read_dir(installation.parent().unwrap().parent().unwrap())
            .unwrap()
            .count()
            == 1,
        "archive removed after extraction"
    );

    // Once selected, a second install from the old root is refused rather than racing.
    let again = install(&anchor, &anchor, "0.10.1", |_| async { Ok(()) }).await;
    assert!(again.is_err());
    assert!(!anchor.join("headless-update.lock").exists());
}

#[tokio::test(flavor = "current_thread")]
async fn failures_leave_the_pointer_and_no_staging_behind() {
    let temp = tempfile::tempdir().unwrap();
    let parent = normalize(temp.path());
    let anchor = parent.join(root_name());
    crate::headless_package::tests::fake_package(&anchor, "0.10.0");
    let result = install(
        &anchor,
        &anchor,
        "0.10.1",
        |destination: PathBuf| async move {
            std::fs::write(destination, b"not an archive")?;
            Ok(())
        },
    )
    .await;
    assert!(result.is_err());
    assert!(!anchor.join(POINTER).exists());
    let leftovers: Vec<_> = std::fs::read_dir(&parent)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(leftovers, [std::ffi::OsString::from(root_name())]);
}

#[test]
fn assets_must_be_the_exact_release_download_with_a_digest() {
    let digest = "a".repeat(64);
    let name = crate::headless_package::asset_name("0.10.0");
    let url = format!("{RELEASES}/download/v0.10.0/{name}");
    let good = serde_json::json!({ "browser_download_url": url, "size": 10, "digest": format!("sha256:{digest}") });
    assert!(parse_asset(&good, "v0.10.0", &name).is_some());
    let mut moved = good.clone();
    moved["browser_download_url"] = "https://example.com/x".into();
    assert!(parse_asset(&moved, "v0.10.0", &name).is_none());
    let mut undigested = good;
    undigested["digest"] = serde_json::Value::Null;
    assert!(parse_asset(&undigested, "v0.10.0", &name).is_none());
    assert!(name.starts_with("shellfox-headless-0.10.0-"));
}
