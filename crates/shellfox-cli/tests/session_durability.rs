//! Every session change is on disk once the host replies: a host killed right
//! after the last change (SIGKILL, TerminateProcess) loses none of them.
#![cfg(any(windows, target_os = "macos", target_os = "linux"))]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
mod support;

use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use shellfox_host::Host;
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn session_changes_survive_a_killed_host() -> Result<()> {
    let (helper, digest) = dev_helper::helper()?;
    let hash: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let tmp = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .context("workspace")?
        .join("tmp");
    std::fs::create_dir_all(&tmp)?;
    let scratch = tempfile::Builder::new()
        .prefix("cli-session-durability-")
        .tempdir_in(&tmp)?;
    let (data, first, second) = (
        scratch.path().join("data"),
        scratch.path().join("first"),
        scratch.path().join("second"),
    );
    for dir in [&data, &first, &second] {
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
    let deadline = Instant::now() + Duration::from_secs(60);
    while json(&["host", "status", "--json"]).is_err() {
        ensure!(
            Instant::now() < deadline && host.0.try_wait()?.is_none(),
            "Host did not start."
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let path = |dir: &Path| dir.to_str().map(str::to_owned).context("Unicode");
    let kept = json(&["session", "create", &path(&first)?, "--json"])?;
    let gone = json(&["session", "create", &path(&second)?, "--json"])?;
    let id = kept["id"].as_str().context("id")?.to_owned();
    json(&["session", "rename", &id, "Renamed", "--json"])?;
    json(&["session", "pin", &id, "--json"])?;
    let env = json!([{"name": "PROJECT", "value": "durable"}]).to_string();
    json(&["session", "env", &id, "--input", &env, "--json"])?;
    json(&["terminals", "create", &id, "--title", "Second", "--json"])?;
    let tabs: Vec<String> = json(&["terminals", "list", &id, "--json"])?
        .as_array()
        .context("tabs")?
        .iter()
        .filter_map(|tab| tab["id"].as_str().map(str::to_owned))
        .collect();
    ensure!(tabs.len() == 2, "Expected two tabs: {tabs:?}");
    json(&["terminals", "rename", &id, &tabs[0], "First", "--json"])?;
    json(&["terminals", "reorder", &id, &tabs[1], &tabs[0], "--json"])?;
    let settings = json!({"accentColor": "#123456"}).to_string();
    json(&["settings", "update", "--input", &settings, "--json"])?;
    let gone = gone["id"].as_str().context("id")?;
    json(&["session", "delete", gone, "--yes", "--json"])?;
    // No graceful shutdown: the host gets no chance to flush anything.
    host.0.kill()?;
    host.wait(Duration::from_secs(20))?;

    let store = open_after_kill(&data)?;
    let sessions = store.snapshot()?.sessions;
    ensure!(sessions.len() == 1, "Expected one session: {sessions:?}");
    let session = &sessions[0];
    ensure!(session.id.to_string() == id, "Wrong session kept");
    ensure!(session.title == "Renamed" && session.pinned_at.is_some());
    ensure!(session.env.len() == 1 && session.env[0].value == "durable");
    let order: Vec<_> = session
        .tabs
        .iter()
        .map(|tab| (tab.id.to_string(), tab.title.as_str()))
        .collect();
    ensure!(
        order.len() == 2 && order[0].0 == tabs[1] && order[1] == (tabs[0].clone(), "First"),
        "Tab order or title lost: {order:?}"
    );
    ensure!(store.global_settings()?.accent_color == "#123456");
    Ok(())
}

/// The killed host's helpers may hold the store lock for a moment.
fn open_after_kill(data: &Path) -> Result<Host> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match Host::open(data) {
            Ok(host) => return Ok(host),
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(error) => return Err(error.into()),
        }
    }
}
