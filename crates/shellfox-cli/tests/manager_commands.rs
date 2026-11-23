//! The 0.9 session, terminals, profiles, path, api, completion and cmd commands
//! through the real CLI against a real host on isolated data (docs/cli.md).
#![cfg(any(windows, target_os = "macos"))]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
#[path = "support/fixture.rs"]
mod fixture;
mod support;

use anyhow::{ensure, Context, Result};
use fixture::Fixture;
use serde_json::Value;

fn code(reply: &(i32, Value)) -> (&str, i32) {
    (reply.1["error"]["code"].as_str().unwrap_or("ok"), reply.0)
}

#[test]
fn session_terminal_and_helper_commands_keep_their_0_9_shapes() -> Result<()> {
    let (helper, digest) = dev_helper::helper()?;
    let host = Fixture::start(helper, digest)?;
    let result = run(&host);
    let stopped = host.stop();
    result?;
    stopped
}

fn run(host: &Fixture) -> Result<()> {
    let project = host.scratch.path().join("project");
    std::fs::create_dir(&project)?;
    let project = project.to_str().context("Unicode")?;
    let session = host.json(&["session", "create", project, "--title", "Backend", "--json"])?;
    let id = session["id"].as_str().context("session id")?.to_owned();
    let id = id.as_str();
    ensure!(
        session["title"] == "Backend" && session["status"].is_string(),
        "{session}"
    );

    // list, inspect: 0.9 tables in plain mode, records with live status in JSON.
    let list = host.json(&["session", "list", "--json"])?;
    ensure!(list.as_array().is_some_and(|s| s
        .iter()
        .any(|s| s["id"] == id && s["counts"]["agents"] == 0)));
    let plain = host.plain(&["ss", "list"])?;
    ensure!(
        plain.starts_with("  NAME") && plain.contains("Backend"),
        "{plain}"
    );
    ensure!(host
        .json(&["session", "list", "--running", "--json"])?
        .as_array()
        .is_some_and(Vec::is_empty));
    ensure!(host.json(&["session", "inspect", id, "--json"])?["cwd"].is_string());
    ensure!(host.plain(&["session", "inspect", id])?.contains("Backend"));

    // Metadata operations return the session, as 0.9's manager methods did.
    ensure!(host.json(&["session", "rename", id, "Renamed", "--json"])?["title"] == "Renamed");
    ensure!(host.json(&["session", "pin", id, "--json"])?["pinnedAt"].is_string());
    ensure!(host.json(&["session", "pin", id, "--off", "--json"])?["pinnedAt"].is_null());
    let env = r#"[{"name":"SHELLFOX_TEST_ENV","value":"one"}]"#;
    ensure!(
        host.json(&["session", "env", id, "--input", env, "--json"])?["env"][0]["value"] == "one"
    );
    ensure!(host.json(&["session", "env", id, "--json"])?[0]["name"] == "SHELLFOX_TEST_ENV");
    let settled = host.json(&["session", "settle", id, "--json"])?;
    ensure!(
        settled["settledAt"].is_string() && settled["status"] == "settled",
        "{settled}"
    );
    ensure!(host
        .plain(&["session", "unsettle", id])?
        .contains("settled At"));
    ensure!(host.json(&["session", "clear-error", id, "--json"])?["id"] == id);
    ensure!(host.json(&["session", "refresh", id, "--json"])?["id"] == id);
    ensure!(code(&host.reply(&["session", "focus", id, "--json"])?) == ("TARGET_LOST", 1));

    // stats and processes: one fresh read, filtered and laid out like control-cli.
    let stats = host.json(&["session", "stats", id, "--json"])?;
    ensure!(
        stats["sessions"][0]["env"] == serde_json::json!([]) && stats["tabs"].is_array(),
        "{stats}"
    );
    ensure!(host
        .plain(&["session", "processes", "--all"])?
        .contains("RSS n/a"));
    let none = host.reply(&["session", "stats", "--json"])?;
    ensure!(
        code(&none) == ("VALIDATION", 2)
            && none.1["error"]["message"]
                .as_str()
                .is_some_and(|m| m.starts_with("No current session."))
    );

    // terminals: create returns the session; list, rename, reorder keep 0.9 arity.
    let before = session["tabs"].as_array().map_or(0, Vec::len);
    let created = host.json(&["terminals", "create", id, "--title", "One", "--json"])?;
    host.json(&[
        "tm", "create", "--id", id, "--title", "Two", "--cwd", project, "--json",
    ])?;
    ensure!(created["tabs"]
        .as_array()
        .is_some_and(|t| t.len() == before + 1));
    let tabs = host.json(&["terminals", "list", id, "--json"])?;
    let ids: Vec<String> = tabs
        .as_array()
        .context("tabs")?
        .iter()
        .map(|t| t["id"].as_str().unwrap_or_default().to_owned())
        .collect();
    ensure!(
        ids.len() == before + 2 && tabs[0]["lifecycle"] == "closed",
        "{tabs}"
    );
    ensure!(host.plain(&["terminals", "list"])?.contains("SESSION"));
    let renamed = host.json(&["terminals", "rename", id, &ids[0], "Alpha", "--json"])?;
    ensure!(renamed["tabs"][0]["title"] == "Alpha");
    let mut order: Vec<&str> = ids.iter().map(String::as_str).rev().collect();
    order.splice(0..0, [id]);
    let mut words = vec!["terminals", "reorder"];
    words.extend(order);
    words.push("--json");
    let reordered = host.json(&words)?;
    ensure!(reordered["tabs"][0]["id"] == ids[ids.len() - 1].as_str());
    // Neither closing nor writing starts a shell that isn't running.
    ensure!(
        code(&host.reply(&["terminals", "write", &ids[0], "x", "--json"])?) == ("NOT_FOUND", 1)
    );
    ensure!(code(&host.reply(&["terminals", "close", &ids[0], "--json"])?) == ("VALIDATION", 2));
    ensure!(
        code(&host.reply(&["terminals", "close", &ids[0], "--yes", "--json"])?) == ("NOT_FOUND", 1)
    );

    // profiles and path helpers.
    ensure!(host.json(&["profiles", "--json"])?["profiles"].is_array());
    ensure!(host.json(&["path", "home", "--json"])?["cwd"].is_string());
    let resolved = host.json(&["path", "resolve", project, "--json"])?;
    ensure!(
        resolved["cwd"]
            .as_str()
            .is_some_and(|c| c.to_lowercase().ends_with("project")),
        "{resolved}"
    );
    let made = host.scratch.path().join("made").join("deep");
    host.json(&[
        "path",
        "create",
        made.to_str().context("Unicode")?,
        "--json",
    ])?;
    ensure!(made.is_dir());
    let prefix = format!(
        "{}{}ma",
        host.scratch.path().display(),
        std::path::MAIN_SEPARATOR
    );
    let matches = host.json(&["path", "complete", &prefix, "--json"])?;
    ensure!(
        matches["matches"].as_array().is_some_and(|m| m.len() == 1),
        "{matches}"
    );

    // api: 0.9 method names and payloads; unknown is VALIDATION, missing is UNSUPPORTED.
    ensure!(host.json(&["api", "getSnapshot", "--json"])?["sessions"].is_array());
    let input = format!(r#"{{"sessionId":"{id}","title":"ByApi"}}"#);
    ensure!(host.json(&["api", "renameSession", "--input", &input, "--json"])?["title"] == "ByApi");
    ensure!(code(&host.reply(&["api", "nope", "--json"])?) == ("VALIDATION", 2));
    ensure!(code(&host.reply(&["api", "installUpdate", "--json"])?) == ("UNSUPPORTED", 1));
    let delete = format!(r#"{{"sessionId":"{id}"}}"#);
    ensure!(
        code(&host.reply(&["api", "deleteSession", "--input", &delete, "--json"])?)
            == ("VALIDATION", 2)
    );
    ensure!(host.json(&["api", "getKeepAwake", "--json"])?["active"].is_boolean());

    // remote api is local network management; keys never come from argv.
    ensure!(host.json(&["remote", "api", "getState", "--json"])?["enabled"] == false);
    let keyed = host.reply(&[
        "remote",
        "api",
        "login",
        "--input",
        r#"{"authKey":"tskey-SECRET"}"#,
        "--json",
    ])?;
    ensure!(code(&keyed) == ("VALIDATION", 2) && !keyed.1.to_string().contains("tskey-SECRET"));
    let unknown = host.reply(&["--host", "no-such-peer", "session", "list", "--json"])?;
    ensure!(
        code(&unknown) == ("VALIDATION", 2)
            && unknown.1["error"]["message"] == "Unknown host. Run shellfox machines list."
    );
    ensure!(code(&host.reply(&["--host", "box", "host", "stop", "--json"])?) == ("VALIDATION", 2));
    // Dev operator words never run here with --host either.
    ensure!(code(&host.reply(&["--host", "box", "tab", "list", "--json"])?) == ("VALIDATION", 2));

    // completion prints the 0.9 hook; cmd keeps the 0.9 registry next to user data.
    ensure!(host
        .plain(&["completion", "bash"])?
        .starts_with("# Shellfox completion for Bash 3.2+."));
    ensure!(host
        .json(&["completion", "zsh", "--json"])?
        .as_str()
        .is_some_and(|s| s.contains("compdef")));
    let completion = host.output(&["completion", "fish"])?;
    ensure!(completion.status.code() == Some(2));
    let user = host.scratch.path().join("user");
    let user = user.to_str().context("Unicode")?;
    let script = host.scratch.path().join(if cfg!(windows) {
        "hello.ps1"
    } else {
        "hello.sh"
    });
    std::fs::write(&script, "exit 4")?;
    let registered = host.json(&[
        "--user-data",
        user,
        "cmd",
        "--register",
        "hello",
        script.to_str().context("Unicode")?,
        "--json",
    ])?;
    ensure!(registered == "Registered \"hello\".", "{registered}");
    ensure!(host.json(&["--user-data", user, "cmd", "--complete", "he", "--json"])? == "hello");
    ensure!(
        host.output(&["--user-data", user, "cmd", "hello", "--json"])?
            .status
            .code()
            == Some(4)
    );

    ensure!(
        host.json(&["session", "delete", id, "--yes", "--json"])?
            == serde_json::json!({ "deleted": true })
    );
    Ok(())
}
