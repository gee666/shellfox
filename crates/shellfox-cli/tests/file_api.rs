//! 0.9 `shellfox api` file and visibility methods against a real host.
#![cfg(any(windows, target_os = "macos", target_os = "linux"))]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
#[path = "support/fixture.rs"]
mod fixture;
mod support;

use anyhow::{ensure, Context, Result};
use fixture::Fixture;
use serde_json::{json, Value};

#[test]
fn file_methods_keep_their_0_9_shapes() -> Result<()> {
    let (helper, digest) = dev_helper::helper()?;
    let host = Fixture::start(helper, digest)?;
    let result = run(&host);
    let stopped = host.stop();
    result?;
    stopped
}

fn api(host: &Fixture, method: &str, input: Value) -> Result<(i32, Value)> {
    host.reply(&["api", method, "--input", &input.to_string(), "--json"])
}
fn ok(host: &Fixture, method: &str, input: Value) -> Result<Value> {
    host.json(&["api", method, "--input", &input.to_string(), "--json"])
}

fn run(host: &Fixture) -> Result<()> {
    let project = host.scratch.path().join("project");
    std::fs::create_dir(&project)?;
    std::fs::write(project.join("notes.txt"), "one\n")?;
    let session = host.json(&[
        "session",
        "create",
        project.to_str().context("Unicode")?,
        "--json",
    ])?;
    let id = session["id"].as_str().context("id")?;

    // Relative paths resolve against the session's folder.
    let listed = ok(host, "listFiles", json!({ "sessionId": id, "path": "." }))?;
    ensure!(
        listed["entries"].as_array().is_some_and(|e| e
            .iter()
            .any(|e| e["name"] == "notes.txt" && e["kind"] == "file")),
        "{listed}"
    );
    let read = ok(
        host,
        "readFile",
        json!({ "sessionId": id, "path": "notes.txt" }),
    )?;
    ensure!(
        read["kind"] == "text" && read["text"] == "one\n" && read["revision"].is_string(),
        "{read}"
    );
    let path = read["path"].as_str().context("path")?;
    let written = ok(
        host,
        "writeFile",
        json!({ "path": path, "text": "two\n", "revision": read["revision"] }),
    )?;
    ensure!(
        written["path"] == path && written["size"] == 4 && written["revision"].is_string(),
        "{written}"
    );
    ensure!(std::fs::read_to_string(project.join("notes.txt"))? == "two\n");
    let stale = api(
        host,
        "writeFile",
        json!({ "path": path, "text": "three\n", "revision": read["revision"] }),
    )?;
    ensure!(
        stale.1["error"]["code"] == "FILE_CONFLICT" && stale.0 == 1,
        "{:?}",
        stale.1
    );
    let missing = api(
        host,
        "readFile",
        json!({ "sessionId": id, "path": "absent.txt" }),
    )?;
    ensure!(missing.1["error"]["code"] == "NOT_FOUND", "{:?}", missing.1);

    ensure!(ok(host, "setVisibleSessions", json!({ "sessionIds": [id] }))?["configured"] == true);
    Ok(())
}
