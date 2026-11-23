//! An endpoint.json this build can't read is another build's host while that
//! host answers, and stale once it has gone.
#![cfg(any(windows, target_os = "macos"))]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
mod support;
use support::OwnedHost;

use anyhow::{ensure, Context, Result};
use shellfox_service::local::{LocalClient, OtherBuildRunning};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn an_unreadable_record_is_another_build_only_while_its_host_answers() -> Result<()> {
    let (helper, digest) = dev_helper::helper()?;
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .context("CLI crate has no workspace root.")?;
    std::fs::create_dir_all(root.join("tmp"))?;
    let scratch = tempfile::Builder::new()
        .prefix("cli-foreign-endpoint-")
        .tempdir_in(root.join("tmp"))?;
    let data = scratch.path().join("data");
    std::fs::create_dir(&data)?;
    let mut host = OwnedHost(
        Command::new(env!("CARGO_BIN_EXE_shellfox-cli"))
            .arg("--data-dir")
            .arg(&data)
            .arg("--helper")
            .arg(&helper)
            .arg("--helper-sha256")
            .arg(hex)
            .args(["host", "serve"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    let endpoint = data.join("rust-service-v1/endpoint.json");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(async {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            ensure!(host.0.try_wait()?.is_none(), "CLI host exited before publishing its endpoint.");
            if endpoint.is_file() && LocalClient::connect(&data, &helper, digest).await.is_ok() {
                break;
            }
            ensure!(Instant::now() < deadline, "Host startup deadline.");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let ours = std::fs::read(&endpoint)?;
        let foreign = older_build(&ours)?;
        std::fs::write(&endpoint, &foreign)?;
        let live = LocalClient::connect(&data, &helper, digest).await;
        let error = live.err().context("An older build's record was accepted.")?;
        ensure!(error.is::<OtherBuildRunning>(), "Live foreign host: {error:#}");
        anyhow::Ok(foreign)
    });
    // A crashed host leaves its record behind. Rewriting the file in place
    // keeps its private permissions.
    host.0.kill()?;
    host.wait(Duration::from_secs(20))?;
    let foreign = result?;
    std::fs::write(&endpoint, &foreign)?;
    let stale = runtime
        .block_on(LocalClient::connect(&data, &helper, digest))
        .err()
        .context("A stale record was accepted.")?;
    ensure!(
        !stale.is::<OtherBuildRunning>() && format!("{stale:#}").contains("stale"),
        "A gone host: {stale:#}"
    );
    Ok(())
}

/// The same record as written by a build from before the namespace rename.
fn older_build(record: &[u8]) -> Result<Vec<u8>> {
    let mut value: serde_json::Value = serde_json::from_slice(record)?;
    value["namespace"] = "shellfox.experimental.rust.v1".into();
    Ok(serde_json::to_vec(&value)?)
}
