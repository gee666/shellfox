//! A tab whose shell exits is removed, as in 0.9.15, so a host restart brings
//! back only the tabs that were still open and starts none of them. A session
//! left without tabs stays, and selecting it opens a fresh shell
//! (docs/database-retention.md).
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
fn exited_tabs_do_not_come_back_after_a_restart() -> Result<()> {
    let (helper, digest) = dev_helper::helper()?;
    let mut host = Fixture::start(helper, digest)?;
    let result = run(&mut host);
    let stopped = host.stop();
    result?;
    stopped
}

fn tabs(host: &Fixture, session: &str) -> Result<Vec<Value>> {
    Ok(host
        .json(&["terminals", "list", session, "--json"])?
        .as_array()
        .cloned()
        .unwrap_or_default())
}
fn ids(tabs: &[Value]) -> Vec<String> {
    tabs.iter()
        .filter_map(|t| t["id"].as_str().map(str::to_owned))
        .collect()
}
fn open(tabs: &[Value]) -> usize {
    tabs.iter().filter(|t| t["lifecycle"] == "open").count()
}
fn wait(what: &str, mut done: impl FnMut() -> Result<bool>) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done()? {
        ensure!(Instant::now() < deadline, "Timed out: {what}");
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}
fn activate(host: &Fixture, session: &str) -> Result<()> {
    let input = json!({ "sessionId": session }).to_string();
    host.json(&["api", "activateSession", "--input", &input, "--json"])?;
    Ok(())
}

fn run(host: &mut Fixture) -> Result<()> {
    let folder = host.scratch.path().join("project");
    std::fs::create_dir(&folder)?;
    let folder = folder.to_str().context("Unicode")?.to_owned();
    let session = |host: &Fixture| -> Result<String> {
        let created = host.json(&["session", "create", &folder, "--json"])?;
        Ok(created["id"].as_str().context("id")?.to_owned())
    };
    let three = session(host)?;
    for _ in 0..2 {
        host.json(&["terminals", "create", &three, "--json"])?;
    }
    let lone = session(host)?;
    let started = ids(&tabs(host, &three)?);
    ensure!(started.len() == 3, "Expected three tabs: {started:?}");
    wait("three shells open", || Ok(open(&tabs(host, &three)?) == 3))?;
    let exited = [&started[0], &started[1], &ids(&tabs(host, &lone)?)[0]];
    for tab in exited {
        host.json(&["terminals", "write", tab, "exit\r", "--json"])?;
    }
    // The exited tabs leave at once, as 0.9 deleted them on exit.
    wait("exited tabs removed", || {
        let left = [tabs(host, &three)?, tabs(host, &lone)?].concat();
        Ok(!ids(&left).iter().any(|id| exited.contains(&id)))
    })?;
    let kept = tabs(host, &three)?;
    ensure!(ids(&kept) == [started[2].clone()], "{kept:?}");
    ensure!(open(&kept) == 1, "The surviving shell stopped: {kept:?}");
    ensure!(tabs(host, &lone)?.is_empty(), "The lone session kept a tab");

    // A clean stop isn't an exit: the open tab comes back, not started.
    host.restart()?;
    let restored = tabs(host, &three)?;
    ensure!(
        ids(&restored) == [started[2].clone()] && open(&restored) == 0,
        "Restart restored {restored:?}"
    );
    let lone_tabs = tabs(host, &lone)?;
    ensure!(lone_tabs.is_empty(), "Restart restored {lone_tabs:?}");

    // Selecting either session opens exactly one shell.
    activate(host, &three)?;
    wait("restored tab opens", || Ok(open(&tabs(host, &three)?) == 1))?;
    ensure!(ids(&tabs(host, &three)?) == [started[2].clone()]);
    activate(host, &lone)?;
    wait("fresh tab opens", || Ok(open(&tabs(host, &lone)?) == 1))?;
    let fresh = tabs(host, &lone)?;
    ensure!(fresh.len() == 1, "{fresh:?}");
    // Selecting it again doesn't open another.
    activate(host, &lone)?;
    ensure!(tabs(host, &lone)?.len() == 1);
    Ok(())
}
