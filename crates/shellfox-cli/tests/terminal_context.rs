//! `shellfox session settle` with no ID, typed into a real host terminal: the
//! tab's SHELLFOX_TERMINAL_MARKER selects its session, as in 0.9. Then
//! `terminals write` refuses an occupied terminal unless --take.
#![cfg(any(windows, target_os = "macos"))]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
#[path = "support/fixture.rs"]
mod fixture;
mod support;

use anyhow::{bail, ensure, Context, Result};
use fixture::Fixture;
use shellfox_service::{
    experiment::{ExperimentRequest, ExperimentResponse},
    local::LocalClient,
};
use std::time::{Duration, Instant};

#[test]
fn a_command_typed_in_a_tab_targets_that_tabs_session() -> Result<()> {
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

fn quote(word: &str) -> String {
    // PowerShell and POSIX shells both read single quotes literally; only the
    // quote itself needs escaping, differently.
    if cfg!(windows) {
        format!("'{}'", word.replace('\'', "''"))
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

async fn run(host: &Fixture, helper: &std::path::Path, digest: [u8; 32]) -> Result<()> {
    let session = host.json(&["session", "create", "--title", "Hooked", "--json"])?;
    let id = session["id"].as_str().context("id")?.to_owned();
    let profile = if cfg!(windows) {
        "windows-powershell"
    } else {
        "zsh"
    };
    let created = host.json(&["terminals", "create", &id, "--profile", profile, "--json"])?;
    let tab = created["tabs"]
        .as_array()
        .and_then(|t| t.last())
        .context("tab")?["id"]
        .as_str()
        .context("tab id")?
        .to_owned();
    let out = host.scratch.path().join("settled.json");

    // Attaching starts the shell; this client then holds its control.
    let client = LocalClient::connect(&host.data, helper, digest).await?;
    let attachment = match client
        .execute(ExperimentRequest::Attach {
            tab: tab.parse()?,
            takeover: false,
        })
        .await?
    {
        ExperimentResponse::Attachment(attachment) => attachment,
        _ => bail!("Unexpected attach response."),
    };
    let mut line: Vec<String> = vec![quote(env!("CARGO_BIN_EXE_shellfox-cli"))];
    line.extend(
        host.argv(&["session", "settle", "--json"])
            .iter()
            .map(|w| quote(w)),
    );
    let command = if cfg!(windows) {
        format!(
            "& {} | Set-Content -Encoding ascii -LiteralPath {}\r",
            line.join(" "),
            quote(&out.display().to_string())
        )
    } else {
        format!(
            "{} > {}\r",
            line.join(" "),
            quote(&out.display().to_string())
        )
    };
    client
        .execute(ExperimentRequest::Input {
            tab: tab.parse()?,
            attachment: attachment.clone(),
            bytes: command.into_bytes(),
        })
        .await?;
    let deadline = Instant::now() + Duration::from_secs(60);
    let reply = loop {
        if let Ok(text) = std::fs::read_to_string(&out) {
            if let Ok(reply) = serde_json::from_str::<serde_json::Value>(text.trim()) {
                break reply;
            }
        }
        ensure!(
            Instant::now() < deadline,
            "The command typed in the tab did not finish."
        );
        // Keep the terminal's output drained while the shell runs.
        let _ = client
            .execute(ExperimentRequest::Poll {
                tab: tab.parse()?,
                attachment: attachment.clone(),
            })
            .await;
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    ensure!(
        reply["ok"] == true && reply["value"]["id"] == id.as_str(),
        "{reply}"
    );
    ensure!(reply["value"]["status"] == "settled", "{reply}");
    ensure!(host.plain(&["session", "list"])?.contains("settled"));

    // The open tab is live now: write joins only when free; --take switches control.
    let held = host.reply(&["terminals", "write", &tab, "echo hi\r", "--json"])?;
    ensure!(
        held.1["error"]["code"] == "TERMINAL_CONTROL_REQUIRED" && held.0 == 1,
        "{:?}",
        held.1
    );
    ensure!(
        host.json(&["terminals", "write", &tab, "echo hi\r", "--take", "--json"])?["written"]
            == true
    );
    ensure!(host.json(&["session", "focus", &id, "--json"])?["focused"] == true);
    let stats = host.json(&["session", "stats", &id, "--json"])?;
    let measured = stats["tabs"]
        .as_array()
        .and_then(|t| t.iter().find(|t| t["tabId"] == tab.as_str()))
        .with_context(|| format!("no stats for the live tab: {stats}"))?;
    // The shell's own process tree, measured on demand.
    ensure!(measured["memoryBytes"].as_u64().is_some_and(|m| m > 0), "{measured}");
    ensure!(measured["elapsedSeconds"].as_f64().is_some_and(|s| s >= 0.0), "{measured}");
    let processes = measured["processes"].as_array().context("processes")?;
    ensure!(
        processes.iter().any(|p| p["pid"].as_u64().is_some_and(|pid| pid > 0)
            && p["parentPid"].as_u64().is_some_and(|pid| pid > 0)),
        "{measured}"
    );
    let tree = host.plain(&["session", "processes", &id])?;
    ensure!(tree.contains("  PID ") && tree.contains(" MiB"), "{tree}");
    let _ = client.disconnect().await;
    Ok(())
}
