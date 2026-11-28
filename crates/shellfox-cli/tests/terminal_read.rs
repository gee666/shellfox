//! `terminals read`, each call its own CLI process against a real host: output
//! positions stay stable across calls, a read never takes control, and a lost
//! cursor is answered with a resumable `replayLost` snapshot.
#![cfg(any(windows, target_os = "macos", target_os = "linux"))]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
#[path = "support/fixture.rs"]
mod fixture;
mod support;

use anyhow::{bail, ensure, Context, Result};
use fixture::Fixture;
use serde_json::{json, Value};
use shellfox_service::{
    local::LocalClient,
    local_api::{LocalApiRequest, LocalApiResponse},
    Attachment,
};
use std::time::{Duration, Instant};

#[test]
fn reads_follow_output_without_taking_control() -> Result<()> {
    let (helper, digest) = dev_helper::helper()?;
    let host = Fixture::start(helper.clone(), digest)?;
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run(&host, &helper, digest));
    let stopped = host.stop();
    result?;
    stopped
}

/// A command printing `count` lines and then `marker`, which its own echo doesn't contain.
fn command(count: u32, marker: &str) -> String {
    if cfg!(windows) {
        format!("1..{count}; echo ('SF'+'{marker}')\r")
    } else {
        format!("seq 1 {count}; echo SF''{marker}\r")
    }
}
fn text(read: &Value) -> String {
    read["chunks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["data"].as_str())
        .collect()
}
fn read(host: &Fixture, tab: &str, after: u64) -> Result<Value> {
    host.json(&[
        "terminals",
        "read",
        tab,
        "--after",
        &after.to_string(),
        "--json",
    ])
}
fn last(read: &Value) -> Result<u64> {
    read["lastSequence"].as_u64().context("lastSequence")
}
/// The blocked attachment's epoch: asking never changes it.
fn epoch(host: &Fixture, tab: &str) -> Result<Value> {
    let blocked = host.json(&[
        "api",
        "attachTerminal",
        "--input",
        &json!({ "tabId": tab }).to_string(),
        "--json",
    ])?;
    ensure!(blocked["control"] == "blocked", "{blocked}");
    Ok(blocked["controlEpoch"].clone())
}

fn snapshot_text(read: &Value) -> String {
    read["snapshotChunks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["data"].as_str())
        .collect()
}

/// Reads after `cursor` until `marker` appears, as a polling bridge would.
/// Returns the new cursor and everything read after the old one. A flood may
/// outrun the replay window; the bridge then resumes from the snapshot.
fn follow(host: &Fixture, tab: &str, cursor: u64, marker: &str) -> Result<(u64, String)> {
    follow_lossy(host, tab, cursor, marker, false)
}
fn follow_lossy(
    host: &Fixture,
    tab: &str,
    mut cursor: u64,
    marker: &str,
    flood: bool,
) -> Result<(u64, String)> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut seen = String::new();
    loop {
        let reply = read(host, tab, cursor)?;
        if flood && reply["replayLost"] == true {
            seen.push_str(&snapshot_text(&reply));
            cursor = last(&reply)?;
            continue;
        }
        ensure!(
            reply.get("replayLost").is_none(),
            "Lost a live cursor: {reply}"
        );
        ensure!(reply["firstSequence"] == cursor + 1, "{reply}");
        let chunks = reply["chunks"].as_array().context("chunks")?;
        ensure!(
            chunks
                .iter()
                .all(|c| c["sequence"].as_u64().is_some_and(|s| s > cursor)),
            "A chunk at or before the cursor came back: {reply}"
        );
        seen.push_str(&text(&reply));
        cursor = last(&reply)?;
        if seen.contains(&format!("SF{marker}")) {
            return Ok((cursor, seen));
        }
        ensure!(
            Instant::now() < deadline,
            "{marker} never appeared: {seen:?}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

async fn attach(client: &LocalClient, tab: &str) -> Result<Attachment> {
    match client
        .execute(LocalApiRequest::Attach {
            tab: tab.parse()?,
            takeover: false,
        })
        .await?
    {
        LocalApiResponse::Attachment(attachment) => Ok(attachment),
        _ => bail!("Unexpected attach response."),
    }
}
async fn type_in(client: &LocalClient, tab: &str, held: &Attachment, text: &str) -> Result<()> {
    client
        .execute(LocalApiRequest::Input {
            tab: tab.parse()?,
            attachment: held.clone(),
            bytes: text.as_bytes().to_vec(),
        })
        .await
        .context("The desktop lost control to a read.")?;
    Ok(())
}

async fn run(host: &Fixture, helper: &std::path::Path, digest: [u8; 32]) -> Result<()> {
    let session = host.json(&["session", "create", "--title", "Read", "--json"])?;
    let tab = session["tabs"][0]["id"].as_str().context("tab")?.to_owned();
    let tab = tab.as_str();

    // The "desktop" holds control; reads still see its output.
    let desktop = LocalClient::connect(&host.data, helper, digest).await?;
    let held = attach(&desktop, tab).await?;
    let before = epoch(host, tab)?;
    let first = host.json(&["terminals", "read", tab, "--json"])?;
    ensure!(
        first["truncated"] == false && first["firstSequence"] == 1,
        "{first}"
    );
    let generation = first["generation"]
        .as_str()
        .context("generation")?
        .to_owned();
    type_in(&desktop, tab, &held, &command(3, "D1")).await?;
    let (mut cursor, _) = follow(host, tab, last(&first)?, "D1")?;
    type_in(&desktop, tab, &held, "\r").await?;
    ensure!(
        epoch(host, tab)? == before,
        "A read changed the control epoch."
    );

    // Writing still needs control, or --take.
    let (code, refused) = host.reply(&["terminals", "write", tab, "echo x\r", "--json"])?;
    ensure!(
        code == 1 && refused["error"]["code"] == "TERMINAL_CONTROL_REQUIRED",
        "{refused}"
    );
    let _ = desktop.disconnect().await;

    // Fill the scrollback, then follow single commands from separate processes.
    host.json(&["terminals", "write", tab, &command(20000, "FULL"), "--json"])?;
    (cursor, _) = follow_lossy(host, tab, cursor, "FULL", true)?;
    let mut previous = "FULL".to_owned();
    for round in 1..=3 {
        let marker = format!("R{round}");
        host.json(&["terminals", "write", tab, &command(1, &marker), "--json"])?;
        let (next, seen) = follow(host, tab, cursor, &marker)?;
        ensure!(
            !seen.contains(&format!("SF{previous}")),
            "Old output came back: {seen:?}"
        );
        (cursor, previous) = (next, marker);
    }

    // A cursor whose output fell out of the replay window gets a snapshot.
    host.json(&[
        "terminals",
        "write",
        tab,
        &command(80000, "FLOOD"),
        "--json",
    ])?;
    let deadline = Instant::now() + Duration::from_secs(120);
    let lost = loop {
        let reply = read(host, tab, cursor)?;
        if reply["replayLost"] == true {
            break reply;
        }
        ensure!(
            Instant::now() < deadline,
            "The stale cursor was never reported."
        );
        std::thread::sleep(Duration::from_millis(200));
    };
    let resume = last(&lost)?;
    ensure!(
        lost["snapshot"] == true
            && lost["truncated"] == true
            && lost["snapshotSequence"] == resume
            && lost["firstSequence"] == resume + 1
            && lost["chunks"].as_array().is_some_and(Vec::is_empty)
            && lost["snapshotChunks"][0]["index"] == 1,
        "{lost}"
    );
    // A fast shell may finish the flood before the loss is seen.
    cursor = if snapshot_text(&lost).contains("SFFLOOD") {
        resume
    } else {
        follow_lossy(host, tab, resume, "FLOOD", true)?.0
    };
    host.json(&["terminals", "write", tab, &command(1, "AFTER"), "--json"])?;
    let (cursor, seen) = follow(host, tab, cursor, "AFTER")?;
    ensure!(!seen.contains("SFFLOOD"), "{seen:?}");

    // Another generation's cursor is lost too; a cursor ahead of output is invalid.
    let replaced = host.json(&[
        "terminals",
        "read",
        tab,
        "--after",
        &cursor.to_string(),
        "--generation",
        "00000000-0000-4000-8000-000000000000",
        "--json",
    ])?;
    ensure!(
        replaced["replayLost"] == true && replaced["generation"] == generation.as_str(),
        "{replaced}"
    );
    let (code, ahead) = host.reply(&[
        "terminals",
        "read",
        tab,
        "--after",
        &(cursor + 1_000_000).to_string(),
        "--json",
    ])?;
    ensure!(
        code == 1 && ahead["error"]["code"] == "VALIDATION",
        "{ahead}"
    );

    // A shell that exits takes its tab with it, as in 0.9: writes to its
    // generation are TARGET_LOST and the tab is gone.
    host.json(&["terminals", "write", tab, "exit\r", "--json"])?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while host
        .json(&["terminals", "list", "--json"])?
        .as_array()
        .into_iter()
        .flatten()
        .any(|t| t["id"] == tab)
    {
        ensure!(Instant::now() < deadline, "The shell never exited.");
        std::thread::sleep(Duration::from_millis(200));
    }
    let write = json!({ "tabId": tab, "generation": generation, "controlEpoch": 1, "data": "x" });
    let (code, lost) = host.reply(&[
        "api",
        "writeTerminal",
        "--input",
        &write.to_string(),
        "--json",
    ])?;
    ensure!(
        code == 1 && lost["error"]["code"] == "TARGET_LOST",
        "{lost}"
    );
    let (code, gone) = host.reply(&["terminals", "read", tab, "--json"])?;
    ensure!(code == 1 && gone["error"]["code"] == "NOT_FOUND", "{gone}");
    Ok(())
}
