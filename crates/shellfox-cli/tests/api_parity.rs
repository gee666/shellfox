//! One-shot `shellfox api` and `settings` calls that 0.9.15 scripts rely on,
//! against a real host: host error codes, createSession retries,
//! activateSession starting a restored shell, the settings round trip and
//! usage errors.
#![cfg(any(windows, target_os = "macos", target_os = "linux"))]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
#[path = "support/fixture.rs"]
mod fixture;
mod support;

use anyhow::{ensure, Context, Result};
use fixture::Fixture;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

#[test]
fn one_shot_api_calls_keep_0_9_behaviour() -> Result<()> {
    let (helper, digest) = dev_helper::helper()?;
    let mut host = Fixture::start(helper, digest)?;
    let result = run(&mut host);
    let stopped = host.stop();
    result?;
    stopped
}

fn api(host: &Fixture, method: &str, input: Value) -> Result<(i32, Value)> {
    host.reply(&["api", method, "--input", &input.to_string(), "--json"])
}
fn failure(reply: &(i32, Value)) -> (&str, i32, &str) {
    let error = &reply.1["error"];
    (
        error["code"].as_str().unwrap_or("ok"),
        reply.0,
        error["message"].as_str().unwrap_or(""),
    )
}
fn tab_open(host: &Fixture, session: &str) -> Result<bool> {
    let tabs = host.json(&["terminals", "list", session, "--json"])?;
    Ok(tabs
        .as_array()
        .is_some_and(|tabs| tabs.iter().any(|t| t["lifecycle"] == "open")))
}

fn run(host: &mut Fixture) -> Result<()> {
    let project = host.scratch.path().join("project");
    std::fs::create_dir(&project)?;
    let project = project.to_str().context("Unicode")?.to_owned();
    let missing = "67e55044-10b1-426f-9247-bb680e5fe0c8";

    // Host refusals keep their code and message, as 0.9 returned them.
    let reply = api(
        host,
        "renameSession",
        json!({"sessionId": missing, "title": "x"}),
    )?;
    ensure!(
        failure(&reply).0 == "NOT_FOUND" && reply.0 == 1,
        "{reply:?}"
    );

    // A retried request ID returns the session it created.
    let request = "0b8f3a52-5a1e-4f43-9d7e-2a64f4f0c001";
    let first = host.json(&[
        "session",
        "create",
        &project,
        "--request-id",
        request,
        "--json",
    ])?;
    let again = host.json(&[
        "session",
        "create",
        &project,
        "--request-id",
        request,
        "--json",
    ])?;
    let retried = api(
        host,
        "createSession",
        json!({"cwd": project, "requestId": request}),
    )?;
    ensure!(
        first["id"].is_string()
            && again["id"] == first["id"]
            && retried.1["value"]["id"] == first["id"],
        "{first} {again} {retried:?}"
    );
    let id = first["id"].as_str().context("id")?.to_owned();
    let reply = api(
        host,
        "reorderTabs",
        json!({"sessionId": id, "tabIds": [missing]}),
    )?;
    ensure!(
        failure(&reply).0 == "VALIDATION" && reply.0 == 1,
        "{reply:?}"
    );

    // Validation names the field, never the rejected value.
    let reply = api(
        host,
        "renameSession",
        json!({"sessionId": id, "title": 7, "x": 1}),
    )?;
    let (code, exit, message) = failure(&reply);
    ensure!(
        code == "VALIDATION" && exit == 2 && message.starts_with("Invalid renameSession input: "),
        "{reply:?}"
    );
    let reply = api(
        host,
        "setSessionEnv",
        json!({"sessionId": id, "env": "secret-value"}),
    )?;
    let message = failure(&reply).2;
    ensure!(
        message.contains("invalid type, expected") && !message.contains("secret-value"),
        "{reply:?}"
    );

    // createDirectory resolves a relative path against the session's folder.
    let created = api(
        host,
        "createDirectory",
        json!({"path": "made", "sessionId": id}),
    )?;
    ensure!(created.0 == 0, "{created:?}");
    ensure!(
        std::path::Path::new(&project).join("made").is_dir(),
        "{created:?}"
    );

    // The printed settings feed straight back into saveSettings and settings update.
    let settings = host.json(&["settings", "read", "--json"])?;
    ensure!(settings.get("pythonPath").is_some(), "{settings}");
    let saved = api(host, "saveSettings", settings.clone())?;
    ensure!(saved.0 == 0, "{saved:?}");
    let file = host.scratch.path().join("settings.json");
    std::fs::write(&file, settings.to_string())?;
    let input = format!("@{}", file.display());
    host.json(&["settings", "update", "--input", &input, "--json"])?;
    let unreadable = format!("@{}", host.scratch.path().join("none.json").display());
    let reply = host.reply(&["settings", "update", "--input", &unreadable, "--json"])?;
    ensure!(
        failure(&reply).0 == "VALIDATION" && reply.0 == 2,
        "{reply:?}"
    );

    // After a restart the shell isn't running; activateSession opens it, as in 0.9.
    host.restart()?;
    ensure!(
        !tab_open(host, &id)?,
        "The restored shell was already running."
    );
    let activated = api(host, "activateSession", json!({"sessionId": id}))?;
    ensure!(activated.0 == 0, "{activated:?}");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !tab_open(host, &id)? {
        ensure!(
            Instant::now() < deadline,
            "activateSession did not start the shell."
        );
        std::thread::sleep(Duration::from_millis(200));
    }

    // Automation argument errors are usage errors.
    let reply = host.reply(&["runs", "show", "nope", "--json"])?;
    ensure!(
        failure(&reply) == ("VALIDATION", 2, "Expected a full run UUID."),
        "{reply:?}"
    );
    Ok(())
}
