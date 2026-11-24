//! A real host on isolated data settles idle sessions and deletes old settled
//! ones on its own, and leaves pinned ones alone (docs/database-retention.md).
//! The stored times are moved back before the host starts; its first sweep
//! runs one minute after start.
#![cfg(any(windows, target_os = "macos", target_os = "linux"))]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
mod support;

use anyhow::{ensure, Context, Result};
use serde_json::Value;
use shellfox_core::{CreateSession, ProfileId, SessionId, SettingsPatch};
use shellfox_host::Host;
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const LONG_AGO: &str = "2020-01-01T00:00:00.000Z";

#[test]
fn the_host_settles_and_deletes_idle_sessions() -> Result<()> {
    let (helper, digest) = dev_helper::helper()?;
    let hash: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .context("workspace")?
        .join("tmp");
    std::fs::create_dir_all(&tmp)?;
    let scratch = tempfile::Builder::new()
        .prefix("cli-session-retention-")
        .tempdir_in(&tmp)?;
    let data = scratch.path().join("data");
    std::fs::create_dir(&data)?;
    let [idle, settled, pinned] = seed(&data)?;

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
        let started = Instant::now();
        while json(&["host", "status", "--json"]).is_err() {
            ensure!(
                started.elapsed() < Duration::from_secs(60) && host.0.try_wait()?.is_none(),
                "Host did not start."
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        let find = |sessions: &Value, id: SessionId| {
            sessions
                .as_array()
                .and_then(|all| all.iter().find(|s| s["id"] == id.to_string()))
                .cloned()
        };
        // Nothing changes before the first sweep.
        let early = json(&["session", "list", "--json"])?;
        ensure!(
            find(&early, idle).is_some_and(|s| s["settledAt"].is_null()),
            "Settled before the first sweep: {early}"
        );
        ensure!(
            find(&early, settled).is_some(),
            "Deleted before the first sweep"
        );
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let sessions = json(&["session", "list", "--json"])?;
            let idle_settled = find(&sessions, idle).is_some_and(|s| s["settledAt"].is_string());
            if idle_settled && find(&sessions, settled).is_none() {
                let kept = find(&sessions, pinned).context("Pinned session deleted")?;
                ensure!(
                    kept["settledAt"].is_null(),
                    "Pinned session settled: {kept}"
                );
                // Settling keeps the session's tab.
                let idle = find(&sessions, idle).context("idle")?;
                ensure!(
                    idle["tabs"].as_array().is_some_and(|tabs| tabs.len() == 1),
                    "Settling changed tabs: {idle}"
                );
                ensure!(
                    started.elapsed() >= Duration::from_secs(55),
                    "Swept before one interval"
                );
                return Ok(());
            }
            ensure!(Instant::now() < deadline, "No sweep: {sessions}");
            std::thread::sleep(Duration::from_secs(1));
        }
    })();
    let stopped = cli(&["host", "stop", "--yes", "--json"]).output();
    let exit = host.wait(Duration::from_secs(20));
    result?;
    ensure!(stopped?.status.success(), "host stop failed");
    ensure!(exit?.success(), "Host exited unsuccessfully.");
    Ok(())
}

/// Three sessions last active long ago, with one-hour settings: one idle,
/// one already settled, one pinned.
fn seed(data: &Path) -> Result<[SessionId; 3]> {
    let host = Host::open(data)?;
    host.save_global_settings(SettingsPatch {
        session_settle_hours: Some(1.0),
        session_delete_hours: Some(1.0),
        ..SettingsPatch::default()
    })?;
    let mut ids = [SessionId::new(); 3];
    for (index, id) in ids.iter_mut().enumerate() {
        let snapshot = host.create_session(CreateSession {
            request_id: SessionId::new(),
            title: Some(format!("Retention {index}")),
            cwd: data.to_string_lossy().into_owned(),
            profile_id: ProfileId::default(),
        })?;
        *id = snapshot
            .sessions
            .iter()
            .find(|s| s.title == format!("Retention {index}"))
            .context("created")?
            .id;
    }
    host.set_session_settled(ids[1], true)?;
    host.set_session_pinned(ids[2], true)?;
    drop(host);
    let db = rusqlite::Connection::open(data.join(shellfox_host::STORE_FILENAME))?;
    db.execute(
        "UPDATE sessions SET value=json_set(value,'$.updatedAt',?1)",
        [LONG_AGO],
    )?;
    db.execute(
        "UPDATE sessions SET value=json_set(value,'$.settledAt',?1) WHERE id=?2",
        [LONG_AGO, &ids[1].to_string()],
    )?;
    Ok(ids)
}
