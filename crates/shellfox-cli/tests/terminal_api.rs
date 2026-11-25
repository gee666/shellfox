//! 0.9 `shellfox api` terminal methods, each from its own one-shot CLI process,
//! against a real host: attachTerminal's snapshot shape, writes, resizes and
//! control. Another local client stands in for the desktop view.
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
fn terminal_api_methods_work_from_one_shot_cli_calls() -> Result<()> {
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

fn api(host: &Fixture, method: &str, input: Value) -> Result<(i32, Value)> {
    host.reply(&["api", method, "--input", &input.to_string(), "--json"])
}
fn ok(host: &Fixture, method: &str, input: Value) -> Result<Value> {
    host.json(&["api", method, "--input", &input.to_string(), "--json"])
}
fn error_code(reply: &(i32, Value)) -> &str {
    reply.1["error"]["code"].as_str().unwrap_or("ok")
}
fn screen(attachment: &Value) -> String {
    attachment["chunks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["data"].as_str())
        .collect()
}

async fn attach(client: &LocalClient, tab: &str, takeover: bool) -> Result<Attachment> {
    match client
        .execute(LocalApiRequest::Attach {
            tab: tab.parse()?,
            takeover,
        })
        .await?
    {
        LocalApiResponse::Attachment(attachment) => Ok(attachment),
        _ => bail!("Unexpected attach response."),
    }
}

async fn run(host: &Fixture, helper: &std::path::Path, digest: [u8; 32]) -> Result<()> {
    let session = host.json(&["session", "create", "--title", "Api", "--json"])?;
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

    // The "desktop" starts the shell and holds its control.
    let desktop = LocalClient::connect(&host.data, helper, digest).await?;
    let held = attach(&desktop, tab, false).await?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let generation = loop {
        let tabs = host.json(&["terminals", "list", &id, "--json"])?;
        let open = tabs.as_array().and_then(|tabs| {
            tabs.iter()
                .find(|t| t["id"] == tab && t["lifecycle"] == "open")
                .map(|t| t["generation"].clone())
        });
        if let Some(generation) = open {
            break generation;
        }
        ensure!(Instant::now() < deadline, "The tab never showed as open.");
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    let identity = json!({ "tabId": tab, "generation": generation });

    // While another view controls it, 0.9 answered "blocked" and refused input.
    let blocked = ok(host, "attachTerminal", json!({ "tabId": tab }))?;
    ensure!(
        blocked["control"] == "blocked"
            && blocked["sessionId"] == id.as_str()
            && blocked["generation"] == generation
            && blocked["controlEpoch"].as_u64().is_some_and(|e| e > 0)
            && blocked.get("chunks").is_none(),
        "{blocked}"
    );
    let mut write = identity.clone();
    write["controlEpoch"] = json!(1);
    write["data"] = json!("echo x\r");
    let refused = api(host, "writeTerminal", write.clone())?;
    ensure!(
        error_code(&refused) == "TERMINAL_CONTROL_REQUIRED" && refused.0 == 1,
        "{:?}",
        refused.1
    );
    // The one-shot calls left the desktop's control alone.
    desktop
        .execute(LocalApiRequest::Resize {
            tab: tab.parse()?,
            attachment: held.clone(),
            cols: 90,
            rows: 25,
        })
        .await
        .context("The desktop lost control to a one-shot call.")?;

    // Stale generations fail as in 0.9.
    let stale = json!({ "tabId": tab, "generation": "00000000-0000-4000-8000-000000000000" });
    ensure!(error_code(&api(host, "attachTerminal", stale.clone())?) == "TARGET_LOST");
    ensure!(error_code(&api(host, "detachTerminal", stale)?) == "TARGET_LOST");

    // takeTerminalControl takes it explicitly; the desktop's lease ends.
    ensure!(ok(host, "takeTerminalControl", identity.clone())?["taken"] == true);
    let revoked = desktop
        .execute(LocalApiRequest::Input {
            tab: tab.parse()?,
            attachment: held,
            bytes: b"x".to_vec(),
        })
        .await;
    ensure!(
        revoked.is_err(),
        "The previous controller could still write."
    );
    drop(desktop);

    // A free terminal: attachTerminal returns the 0.9 snapshot.
    let owned = ok(host, "attachTerminal", identity.clone())?;
    let chunks = owned["chunks"].as_array().context("chunks")?;
    ensure!(
        owned["control"] == "owned"
            && owned["controlEpoch"].as_u64().is_some_and(|e| e > 0)
            && owned["generation"] == generation
            && owned["sessionId"] == id.as_str()
            && owned["snapshot"] == true
            && owned["state"] == "open"
            && owned["lifetime"] == "app-owned"
            && owned["firstSequence"] == 1
            && owned["lastSequence"] == chunks.len()
            && owned["exitCode"].is_null()
            && owned.get("settledAt").is_some()
            && owned["cols"] == 90
            && owned["rows"] == 25,
        "{owned}"
    );
    ensure!(chunks
        .iter()
        .all(|c| c["type"] == "data" && c["tabId"] == tab));

    // writeTerminal with the epoch from that attach, in a new process.
    let (command, output) = if cfg!(windows) {
        ("echo ('SF'+'API')\r", "SFAPI")
    } else {
        ("echo SF''API\r", "SFAPI")
    };
    write["controlEpoch"] = owned["controlEpoch"].clone();
    write["data"] = json!(command);
    ensure!(ok(host, "writeTerminal", write)?["written"] == true);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let attached = ok(host, "attachTerminal", json!({ "tabId": tab }))?;
        if screen(&attached)
            .lines()
            .any(|line| line.trim_end().ends_with(output))
        {
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "The written command's output never appeared: {:?}",
            screen(&attached)
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let mut resize = identity.clone();
    resize["controlEpoch"] = json!(1);
    resize["cols"] = json!(100);
    resize["rows"] = json!(30);
    ensure!(ok(host, "resizeTerminal", resize)?["resized"] == true);
    let resized = ok(host, "attachTerminal", json!({ "tabId": tab }))?;
    ensure!(resized["cols"] == 100 && resized["rows"] == 30, "{resized}");

    let mut ack = identity.clone();
    ack["controlEpoch"] = json!(1);
    ack["sequence"] = json!(1);
    ensure!(ok(host, "acknowledgeTerminal", ack)?["acknowledged"] == true);
    ensure!(ok(host, "detachTerminal", identity)?["detached"] == true);

    // Each call's control ended with its process: a new view joins without takeover.
    let desktop = LocalClient::connect(&host.data, helper, digest).await?;
    attach(&desktop, tab, false)
        .await
        .context("A one-shot call kept the terminal's control.")?;
    let _ = desktop.disconnect().await;
    Ok(())
}
