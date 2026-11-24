//! The first attach starts a tab's shell. Right after that client detaches,
//! the tab lists as open and `terminals write` and `terminals close` find it.
#![cfg(any(windows, target_os = "macos", target_os = "linux"))]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
#[path = "support/fixture.rs"]
mod fixture;
mod support;

use anyhow::{bail, ensure, Context, Result};
use fixture::Fixture;
use shellfox_service::{
    local::LocalClient,
    local_api::{LocalApiRequest, LocalApiResponse},
};
use std::time::{Duration, Instant};

#[test]
fn a_detach_never_makes_an_open_tab_look_closed() -> Result<()> {
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

/// What `shellfox terminals attach` does: attach, then leave with a detach.
async fn attach_and_detach(
    host: &Fixture,
    helper: &std::path::Path,
    digest: [u8; 32],
    tab: &str,
) -> Result<()> {
    let client = LocalClient::connect(&host.data, helper, digest).await?;
    let attached = client
        .execute(LocalApiRequest::Attach {
            tab: tab.parse()?,
            takeover: false,
        })
        .await?;
    if !matches!(attached, LocalApiResponse::Attachment(_)) {
        bail!("Unexpected attach response.");
    }
    client.disconnect().await
}

fn lifecycle(host: &Fixture, session: &str, tab: &str) -> Result<String> {
    let tabs = host.json(&["terminals", "list", session, "--json"])?;
    Ok(tabs
        .as_array()
        .and_then(|t| t.iter().find(|t| t["id"] == tab))
        .context("listed tab")?["lifecycle"]
        .as_str()
        .context("lifecycle")?
        .to_owned())
}

async fn run(host: &Fixture, helper: &std::path::Path, digest: [u8; 32]) -> Result<()> {
    let session = host.json(&["session", "create", "--title", "Detach", "--json"])?;
    let id = session["id"].as_str().context("id")?.to_owned();
    let started = Instant::now();
    for round in 0..50 {
        let created = host.json(&["terminals", "create", &id, "--json"])?;
        let tab = created["tabs"]
            .as_array()
            .and_then(|t| t.last())
            .context("tab")?["id"]
            .as_str()
            .context("tab id")?
            .to_owned();
        attach_and_detach(host, helper, digest, &tab).await?;
        let state = lifecycle(host, &id, &tab)?;
        ensure!(state == "open", "round {round}: listed as {state}");
        let written = host.reply(&["terminals", "write", &tab, "\r", "--json"])?;
        ensure!(
            written.0 == 0 && written.1["value"]["written"] == true,
            "round {round}: write {:?}",
            written.1
        );
        attach_and_detach(host, helper, digest, &tab).await?;
        let closed = host.reply(&["terminals", "close", &tab, "--yes", "--json"])?;
        ensure!(closed.0 == 0, "round {round}: close {:?}", closed.1);
    }
    // No step waits on a held activity read (25 s each).
    ensure!(
        started.elapsed() < Duration::from_secs(240),
        "50 rounds took {:?}",
        started.elapsed()
    );
    Ok(())
}
