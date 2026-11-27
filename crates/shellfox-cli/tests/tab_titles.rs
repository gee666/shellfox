//! A tab without an explicit name shows the title its program publishes with
//! OSC 0/2, in every client's records, until the user renames it (0.9 rule).
#![cfg(any(windows, target_os = "macos", target_os = "linux"))]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
#[path = "support/fixture.rs"]
mod fixture;
mod support;

use anyhow::{bail, ensure, Context, Result};
use fixture::Fixture;
use serde_json::Value;
use shellfox_service::{
    local::LocalClient,
    local_api::{LocalApiRequest, LocalApiResponse},
    Attachment,
};
use std::time::{Duration, Instant};

#[test]
fn published_titles_name_automatic_tabs_until_renamed() -> Result<()> {
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

/// The shell command that makes the terminal publish `title`.
fn set_title(title: &str) -> String {
    if cfg!(windows) {
        // ConPTY forwards the console title as OSC 0.
        format!("$Host.UI.RawUI.WindowTitle = '{title}'\r")
    } else {
        format!("printf '\\033]2;%s\\007' '{title}'\r")
    }
}

/// The tab's title in the host's session records and in the 0.9 snapshot
/// remote clients read.
fn titles(host: &Fixture, session: &str, tab: &str) -> Result<[String; 2]> {
    let find = |tabs: &Value| {
        tabs.as_array()
            .and_then(|tabs| tabs.iter().find(|t| t["id"] == tab))
            .and_then(|t| t["title"].as_str())
            .unwrap_or_default()
            .to_owned()
    };
    let listed = host.json(&["terminals", "list", session, "--json"])?;
    let snapshot = host.json(&["api", "getSnapshot", "--json"])?;
    let sessions = snapshot["sessions"].as_array().context("sessions")?;
    let session = sessions
        .iter()
        .find(|s| s["id"] == session)
        .context("session")?;
    Ok([find(&listed), find(&session["tabs"])])
}

async fn wait_for(host: &Fixture, session: &str, tab: &str, title: &str) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let seen = titles(host, session, tab)?;
        if seen.iter().all(|seen| seen == title) {
            return Ok(());
        }
        ensure!(
            Instant::now() < deadline,
            "Expected {title:?}, saw {seen:?}."
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn input(client: &LocalClient, tab: &str, held: &Attachment, text: &str) -> Result<()> {
    client
        .execute(LocalApiRequest::Input {
            tab: tab.parse()?,
            attachment: held.clone(),
            bytes: text.as_bytes().to_vec(),
        })
        .await?;
    Ok(())
}

async fn run(host: &Fixture, helper: &std::path::Path, digest: [u8; 32]) -> Result<()> {
    let session = host.json(&["session", "create", "--title", "Titles", "--json"])?;
    let id = session["id"].as_str().context("id")?.to_owned();
    let profile = if cfg!(windows) {
        "windows-powershell"
    } else if cfg!(target_os = "macos") {
        "zsh"
    } else {
        "bash"
    };
    let created = host.json(&["terminals", "create", &id, "--profile", profile, "--json"])?;
    let tab = created["tabs"]
        .as_array()
        .and_then(|t| t.last())
        .context("tab")?["id"]
        .as_str()
        .context("tab id")?
        .to_owned();
    let tab = tab.as_str();
    let saved = titles(host, &id, tab)?;
    ensure!(saved.iter().all(|t| t.starts_with("Shell ")), "{saved:?}");

    // A view starts the shell and types for it.
    let client = LocalClient::connect(&host.data, helper, digest).await?;
    let held = match client
        .execute(LocalApiRequest::Attach {
            tab: tab.parse()?,
            takeover: false,
        })
        .await?
    {
        LocalApiResponse::Attachment(attachment) => attachment,
        _ => bail!("Unexpected attach response."),
    };
    input(&client, tab, &held, &set_title("Agent at work")).await?;
    wait_for(host, &id, tab, "Agent at work").await?;

    // An explicit name wins over every later published title.
    host.json(&["terminals", "rename", &id, tab, "Mine", "--json"])?;
    input(&client, tab, &held, &set_title("Agent again")).await?;
    input(&client, tab, &held, &set_title("Agent done")).await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let renamed = titles(host, &id, tab)?;
    ensure!(renamed.iter().all(|t| t == "Mine"), "{renamed:?}");
    Ok(())
}
