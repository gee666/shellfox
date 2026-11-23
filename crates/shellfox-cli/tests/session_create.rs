//! `shellfox session create [PATH]` through the real CLI against a real host
//! on isolated data, then `session list` shows it (docs/cli.md#session).
#![cfg(any(windows, target_os = "macos"))]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
mod support;

use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn session_create_with_a_positional_path_or_the_caller_directory() -> Result<()> {
    let (helper, digest) = dev_helper::helper()?;
    let hash: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .context("workspace")?
        .join("tmp");
    std::fs::create_dir_all(&tmp)?;
    let scratch = tempfile::Builder::new()
        .prefix("cli-session-create-")
        .tempdir_in(&tmp)?;
    let (data, project, caller) = (
        scratch.path().join("data"),
        scratch.path().join("project"),
        scratch.path().join("caller"),
    );
    for dir in [&data, &project, &caller] {
        std::fs::create_dir(dir)?;
    }
    let cli = |args: &[&str]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_shellfox-cli"));
        command
            .arg("--data-dir")
            .arg(&data)
            .arg("--helper")
            .arg(&helper)
            .arg("--helper-sha256")
            .arg(&hash)
            .args(args)
            .current_dir(&caller)
            .stdin(Stdio::null());
        command
    };
    let json = |args: &[&str]| -> Result<Value> {
        let output = cli(args).output()?;
        let reply: Value = serde_json::from_slice(&output.stdout)
            .with_context(|| format!("{args:?}: {}", String::from_utf8_lossy(&output.stderr)))?;
        ensure!(
            output.status.success() && reply["ok"] == true,
            "{args:?} failed: {reply}"
        );
        Ok(reply["value"].clone())
    };
    let mut host = support::OwnedHost(
        cli(&["host", "run", "--json"])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    let result = (|| -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(60);
        while json(&["host", "status", "--json"]).is_err() {
            ensure!(
                Instant::now() < deadline && host.0.try_wait()?.is_none(),
                "Host did not start."
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        let named = json(&[
            "session",
            "create",
            project.to_str().context("Unicode")?,
            "--title",
            "Backend",
            "--json",
        ])?;
        ensure!(named["title"] == "Backend", "Unexpected session: {named}");
        let plain = cli(&["session", "create"]).output()?;
        ensure!(
            plain.status.success(),
            "Plain create failed: {}",
            String::from_utf8_lossy(&plain.stderr)
        );
        let sessions = json(&["session", "list", "--json"])?;
        let cwds: Vec<String> = sessions
            .as_array()
            .context("list")?
            .iter()
            .map(|s| s["cwd"].as_str().unwrap_or_default().to_lowercase())
            .collect();
        let canonical = |path: &Path| dunce(path).to_lowercase();
        ensure!(
            cwds.contains(&canonical(&project)),
            "Named session missing: {cwds:?}"
        );
        ensure!(
            cwds.contains(&canonical(&caller)),
            "Default-cwd session missing: {cwds:?}"
        );
        ensure!(json(&["session", "list", "--json"])?
            .as_array()
            .is_some_and(|s| s.iter().any(|s| s["id"] == named["id"])));
        Ok(())
    })();
    let stopped = cli(&["host", "stop", "--yes", "--json"]).output();
    let exit = host.wait(Duration::from_secs(20));
    result?;
    ensure!(stopped?.status.success(), "host stop failed");
    ensure!(exit?.success(), "Host exited unsuccessfully.");
    Ok(())
}

/// The host stores the real path without Windows' `\\?\` prefix.
fn dunce(path: &Path) -> String {
    let real = std::fs::canonicalize(path)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    real.strip_prefix(r"\\?\")
        .map(str::to_owned)
        .unwrap_or(real)
}
