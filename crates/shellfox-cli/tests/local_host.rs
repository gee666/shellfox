#![cfg(any(windows, target_os = "macos"))]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
mod support;
use support::OwnedHost;

use anyhow::{bail, ensure, Context, Result};
use shellfox_core::{CreateSession, ProfileId, SessionId, TabId};
use shellfox_service::{
    experiment::{
        ExperimentRequest as Request, ExperimentResponse as Response, RecoveryCapability,
    },
    local::LocalClient,
    Attachment, TerminalOutput,
};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn owned_host_pty_input_takeover_disconnect_and_stop() -> Result<()> {
    let (helper, digest) = dev_helper::helper()?;
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .context("CLI crate has no workspace root.")?;
    std::fs::create_dir_all(root.join("tmp"))?;
    let scratch = tempfile::Builder::new()
        .prefix("cli-owned-host-")
        .tempdir_in(root.join("tmp"))?;
    let data = scratch.path().join("data");
    std::fs::create_dir(&data)?;
    let mut host = OwnedHost(
        Command::new(env!("CARGO_BIN_EXE_shellfox-cli"))
            .arg("--data-dir")
            .arg(&data)
            .arg("--helper")
            .arg(&helper)
            .arg("--helper-sha256")
            .arg(hex)
            .args(["host", "serve"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(exercise(&mut host, &data, &helper, digest, scratch.path()));
    if result.is_err() {
        // Try normal typed shutdown even after an assertion fails. Drop only kills
        // this owned child if no typed cleanup can finish.
        runtime.block_on(async {
            if let Ok(client) = LocalClient::connect(&data, &helper, digest).await {
                let _ = client.execute(Request::Stop {}).await;
            }
        });
    }
    let exit = host.wait(Duration::from_secs(20));
    result?;
    ensure!(exit?.success(), "CLI host exited unsuccessfully.");
    Ok(())
}

async fn exercise(
    host: &mut OwnedHost,
    data: &Path,
    helper: &Path,
    digest: [u8; 32],
    cwd: &Path,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let endpoint = data.join("rust-service-v1/endpoint.json");
    let first = loop {
        ensure!(
            host.0.try_wait()?.is_none(),
            "CLI host exited before publishing its endpoint."
        );
        if !endpoint.is_file() && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
            continue;
        }
        match LocalClient::connect(data, helper, digest).await {
            Ok(client) => break client,
            Err(error) if Instant::now() >= deadline => {
                return Err(error.context("Host startup deadline."))
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    };
    let cli = Path::new(env!("CARGO_BIN_EXE_shellfox-cli"));
    let saved = support::settings(
        cli,
        data,
        helper,
        digest,
        &["settings", "update", "--input", r#"{"historyPageSize":17}"#, "--json"],
    )?;
    ensure!(
        saved.history_page_size == 17,
        "CLI did not save full global settings."
    );
    let loaded = support::settings(cli, data, helper, digest, &["settings", "read", "--json"])?;
    ensure!(
        loaded == saved,
        "CLI settings read did not return the committed global settings."
    );
    let snapshot = match first
        .execute(Request::CreateSession {
            input: CreateSession {
                request_id: SessionId::new(),
                title: Some("CLI owned-host test".into()),
                cwd: cwd.to_str().context("Test path must be UTF-8.")?.into(),
                profile_id: ProfileId::new(if cfg!(windows) { "cmd" } else { "login-shell" })?,
            },
        })
        .await?
    {
        Response::Snapshot(snapshot) => snapshot,
        _ => bail!("Invalid create response."),
    };
    let tab = snapshot
        .selection
        .tab_id
        .context("New session has no selected tab.")?;
    let attachment = attach(&first, tab, false).await?;
    let sequence = recover(&first, tab, &attachment).await?;
    send_marker(&first, tab, &attachment, sequence).await?;

    let second = LocalClient::connect(data, helper, digest).await?;
    let replacement = attach(&second, tab, true).await?;
    ensure!(
        attachment.generation == replacement.generation,
        "Takeover restarted the terminal."
    );
    ensure!(
        replacement.epoch > attachment.epoch,
        "Takeover did not advance the epoch."
    );
    ensure!(
        first
            .execute(Request::Input {
                tab,
                attachment: attachment.clone(),
                bytes: b"wrong\r".to_vec()
            })
            .await
            .is_err(),
        "A stale controller was allowed to send input."
    );
    first.disconnect().await?;
    let sequence = recover(&second, tab, &replacement).await?;
    // Old-client disconnect must leave the same owned process usable.
    send_marker(&second, tab, &replacement, sequence).await?;
    second.disconnect().await?;

    let third = LocalClient::connect(data, helper, digest).await?;
    let reattached = attach(&third, tab, false).await?;
    ensure!(
        reattached.generation == replacement.generation,
        "Clean detach restarted the terminal."
    );
    ensure!(
        reattached.epoch > replacement.epoch,
        "Reattach did not advance the epoch."
    );
    let stale_input = Request::Input {
        tab,
        attachment: replacement,
        bytes: b"wrong\r".to_vec(),
    };
    ensure!(
        second.execute(stale_input).await.is_err(),
        "A detached controller was allowed to send input."
    );
    let sequence = recover(&third, tab, &reattached).await?;
    send_marker(&third, tab, &reattached, sequence).await?;
    match third.execute(Request::CloseTab { id: tab }).await? {
        Response::Snapshot(snapshot) => ensure!(
            snapshot
                .sessions
                .iter()
                .flat_map(|session| &session.tabs)
                .all(|value| value.id != tab),
            "Closed tab metadata survived confirmed process cleanup."
        ),
        _ => bail!("Invalid close response."),
    }
    done(third.execute(Request::Stop {}).await?)
}

async fn attach(client: &LocalClient, tab: TabId, takeover: bool) -> Result<Attachment> {
    match client.execute(Request::Attach { tab, takeover }).await? {
        Response::Attachment(value) => Ok(value),
        _ => bail!("Invalid attachment response."),
    }
}

async fn recover(client: &LocalClient, tab: TabId, attachment: &Attachment) -> Result<u64> {
    match client
        .execute(Request::Recover {
            tab,
            attachment: attachment.clone(),
        })
        .await?
    {
        Response::Recovery {
            bytes,
            last_sequence,
            capability: RecoveryCapability::VisibleScreen,
        } => {
            ensure!(!bytes.is_empty(), "Recovery should contain an ANSI screen.");
            ack(client, tab, attachment, last_sequence).await?;
            Ok(last_sequence)
        }
        _ => bail!("Invalid recovery response."),
    }
}

async fn send_marker(
    client: &LocalClient,
    tab: TabId,
    attachment: &Attachment,
    mut sequence: u64,
) -> Result<()> {
    let suffix = SessionId::new().to_string();
    let marker = format!("SHELLFOX_CLI_TYPED_{suffix}");
    // The expected contiguous marker does not occur in echoed input text.
    let command = if cfg!(windows) {
        format!(
            "set SF_E2E_A=SHELLFOX_CLI_TYPED_\rset SF_E2E_B={suffix}\recho %SF_E2E_A%%SF_E2E_B%\r"
        )
    } else {
        format!("printf '%s%s\\n' SHELLFOX_CLI_TYPED_ '{suffix}'\r")
    };
    done(
        client
            .execute(Request::Input {
                tab,
                attachment: attachment.clone(),
                bytes: command.into_bytes(),
            })
            .await?,
    )?;
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut output = Vec::new();
    let mut replay_checked = false;
    loop {
        let chunks = poll(client, tab, attachment).await?;
        if !chunks.is_empty() && !replay_checked {
            let repeated = poll(client, tab, attachment).await?;
            for chunk in &chunks {
                ensure!(repeated.iter().any(|value| value.sequence == chunk.sequence && value.bytes == chunk.bytes),
                    "Poll discarded output before acknowledgement.");
            }
            replay_checked = true;
        }
        for chunk in chunks {
            if chunk.sequence <= sequence {
                continue;
            }
            ensure!(
                sequence.checked_add(1) == Some(chunk.sequence),
                "Live output has a sequence gap."
            );
            output.extend_from_slice(&chunk.bytes);
            sequence = chunk.sequence;
        }
        ack(client, tab, attachment, sequence).await?;
        if output
            .windows(marker.len())
            .any(|value| value == marker.as_bytes())
        {
            ensure!(replay_checked, "Output replay check was skipped.");
            return Ok(());
        }
        ensure!(
            output.len() <= 2 * 1024 * 1024,
            "Unexpected terminal output volume."
        );
        ensure!(
            Instant::now() < deadline,
            "Typed PTY marker was not returned by live Poll."
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn poll(
    client: &LocalClient,
    tab: TabId,
    attachment: &Attachment,
) -> Result<Vec<TerminalOutput>> {
    match client
        .execute(Request::Poll {
            tab,
            attachment: attachment.clone(),
        })
        .await?
    {
        Response::Output(chunks) => Ok(chunks),
        _ => bail!("Invalid output response."),
    }
}

async fn ack(
    client: &LocalClient,
    tab: TabId,
    attachment: &Attachment,
    sequence: u64,
) -> Result<()> {
    done(
        client
            .execute(Request::Acknowledge {
                tab,
                attachment: attachment.clone(),
                sequence,
            })
            .await?,
    )
}
fn done(response: Response) -> Result<()> {
    ensure!(
        matches!(response, Response::Done {}),
        "Invalid command response."
    );
    Ok(())
}
