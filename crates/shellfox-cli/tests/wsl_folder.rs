//! On Windows, a WSL folder opens in its distro (0.9 TerminalService.profile):
//! `session create`, `terminals create` without --profile and `path complete`
//! on the share, through the real CLI against a real host. Needs the Debian distro.
#![cfg(windows)]
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

const SHARE: &str = r"\\wsl.localhost\Debian\tmp";

#[test]
fn wsl_share_sessions_and_tabs_default_to_the_distro_profile() -> Result<()> {
    if !Path::new(SHARE).is_dir() {
        eprintln!("skipped: no Debian WSL share");
        return Ok(());
    }
    let (helper, digest) = dev_helper::helper()?;
    let hash: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .context("workspace")?
        .join("tmp");
    std::fs::create_dir_all(&tmp)?;
    let scratch = tempfile::Builder::new()
        .prefix("cli-wsl-folder-")
        .tempdir_in(&tmp)?;
    let data = scratch.path().join("data");
    std::fs::create_dir(&data)?;
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
            .current_dir(scratch.path())
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
        let matches = json(&["path", "complete", "//wsl.localhost/Debian/tm", "--json"])?;
        ensure!(
            matches["matches"]
                .as_array()
                .is_some_and(|m| m.iter().any(|m| m == "//wsl.localhost/Debian/tmp/")),
            "Unexpected completions: {matches}"
        );
        let resolved = json(&["path", "resolve", "//wsl.localhost/Debian/tmp", "--json"])?;
        ensure!(resolved["cwd"] == SHARE, "Unexpected resolve: {resolved}");

        // WSL discovery runs after start; the first create may precede it.
        let deadline = Instant::now() + Duration::from_secs(90);
        let session = loop {
            match json(&["session", "create", SHARE, "--json"]) {
                Ok(session) => break session,
                Err(error) if Instant::now() < deadline => {
                    eprintln!("retrying: {error:#}");
                    std::thread::sleep(Duration::from_secs(1));
                }
                Err(error) => return Err(error),
            }
        };
        let profiles = |session: &Value| -> Vec<String> {
            session["tabs"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|tab| tab["profileId"].as_str().unwrap_or_default().to_owned())
                .collect()
        };
        ensure!(
            profiles(&session) == ["wsl:Debian"],
            "First tab profile: {session}"
        );
        let id = session["id"].as_str().context("session id")?;
        let added = json(&["terminals", "create", "--id", id, "--json"])?;
        ensure!(
            profiles(&added) == ["wsl:Debian", "wsl:Debian"],
            "Added tab profile: {added}"
        );
        Ok(())
    })();
    let stopped = cli(&["host", "stop", "--yes", "--json"]).output();
    let exit = host.wait(Duration::from_secs(20));
    result?;
    ensure!(stopped?.status.success(), "host stop failed");
    ensure!(exit?.success(), "Host exited unsuccessfully.");
    Ok(())
}
