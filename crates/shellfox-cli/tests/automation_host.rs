#![cfg(windows)]
#[path = "support/automation_lifetime.rs"]
mod lifetime;
#[path = "support/mod.rs"]
pub mod support;
use anyhow::{ensure, Context, Result};
use lifetime::actual_cli_lifetime;
use shellfox_service::{
    experiment::ExperimentRequest,
    local::{automation::*, LocalClient},
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[test]
#[ignore = "explicit trusted helper+owned raw fixture only; no installed host/data"]
fn same_client_deferred_stdin_and_consumption_acked_output_have_no_head_of_line_block() -> Result<()>
{
    let helper = PathBuf::from(
        std::env::var_os("SHELLFOX_CLI_TEST_HELPER").context("explicit verified helper")?,
    );
    let raw = PathBuf::from(
        std::env::var_os("SHELLFOX_RAW_TEST_HELPER").context("explicit owned raw fixture")?,
    );
    ensure!(
        helper.is_absolute() && helper.is_file() && raw.is_absolute() && raw.is_file(),
        "Existing absolute fixture paths required."
    );
    let hex = std::env::var("SHELLFOX_CLI_TEST_HELPER_SHA256")?;
    ensure!(
        hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Trusted digest required."
    );
    let mut digest = [0; 32];
    for (i, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)?;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root)?;
    let scratch = tempfile::Builder::new()
        .prefix("cli-automation-owner-")
        .tempdir_in(root)?;
    let data = scratch.path().join("data");
    std::fs::create_dir(&data)?;
    let mut host = support::OwnedHost(
        Command::new(env!("CARGO_BIN_EXE_shellfox-cli"))
            .args([
                "--data-dir",
                data.to_str().context("Unicode fixture path")?,
                "--helper",
                helper.to_str().context("Unicode helper")?,
                "--helper-sha256",
                &hex,
                "host",
                "serve",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(async {
        let deadline = Instant::now() + Duration::from_secs(20);
        let client = loop {
            ensure!(
                host.0.try_wait()?.is_none(),
                "Owned host exited during startup."
            );
            if let Ok(client) = LocalClient::connect(&data, &helper, digest).await {
                break client;
            }
            ensure!(
                Instant::now() < deadline,
                "Owned private endpoint startup timed out."
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let spec = AutomationStart {
            request_id: uuid::Uuid::new_v4(),
            executable: raw.to_str().context("Unicode raw fixture")?.into(),
            args: vec!["prefill-echo".into()],
            cwd: scratch.path().to_str().context("Unicode cwd")?.into(),
            env: BTreeMap::new(),
            title: Some("Owned deferred IO fixture".into()),
            detached: false,
        };
        let run = client.request(&spec).await?;
        ensure!(
            client.request(&spec).await?.pid == run.pid,
            "Request-ID retry created another owned process."
        );
        let input: Vec<u8> = (0..AUTOMATION_CHUNK_BYTES)
            .map(|i| (i % 256) as u8)
            .collect();
        let written = Arc::new(AtomicBool::new(false));
        let writing = written.clone();
        let write = async {
            let receipt = client
                .request(&AutomationWrite {
                    id: run.id,
                    data: shellfox_service::local::encode_chunk(&input)?,
                    eof: true,
                })
                .await?;
            writing.store(true, Ordering::Release);
            ensure!(receipt.accepted, "OS write was not accepted.");
            Ok::<_, anyhow::Error>(())
        };
        let read = async {
            let (mut cursor, mut stdout, mut stderr) = (0, 0usize, 0usize);
            let mut progressed_while_pending = false;
            loop {
                let output = client
                    .request(&AutomationRead {
                        id: run.id,
                        after: cursor,
                    })
                    .await?;
                let empty = output.frames.is_empty();
                for frame in output.frames {
                    ensure!(
                        frame.sequence > cursor,
                        "Output was duplicated or reordered."
                    );
                    let bytes = shellfox_service::local::decode_chunk(&frame.data)?;
                    match frame.stream {
                        AutomationStream::Stdout => {
                            if !written.load(Ordering::Acquire) {
                                progressed_while_pending = true;
                            }
                            for byte in bytes {
                                let expected = if stdout < 2 * 1024 * 1024 {
                                    123
                                } else {
                                    *input
                                        .get(stdout - 2 * 1024 * 1024)
                                        .context("Unexpected extra stdout")?
                                };
                                ensure!(byte == expected, "Byte-exact pipeline output changed.");
                                stdout += 1;
                            }
                        }
                        AutomationStream::Stderr => {
                            for byte in bytes {
                                ensure!(
                                    Some(&byte) == [0, 255, 10].get(stderr),
                                    "Stderr sentinel changed."
                                );
                                stderr += 1;
                            }
                        }
                    }
                    // Advance only after the consumer verified the real bytes.
                    cursor = frame.sequence;
                }
                if output.run.state == AutomationState::Exited && empty {
                    ensure!(output.run.exit_code == Some(7), "Raw exit code changed.");
                    ensure!(
                        stdout == 2 * 1024 * 1024 + input.len() && stderr == 3,
                        "Final pipeline byte count changed."
                    );
                    ensure!(
                        progressed_while_pending,
                        "Fixture did not exercise deferred write/read overlap."
                    );
                    return Ok::<_, anyhow::Error>(());
                }
                if empty {
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(20), async {
            tokio::try_join!(write, read)
        })
        .await
        .context("Listener/owner head-of-line blocked deferred pipes")??;
        actual_cli_lifetime(&client, &data, &helper, &hex, &raw, scratch.path()).await?;
        client.disconnect().await?;
        Ok::<_, anyhow::Error>(())
    });
    let stopped = runtime.block_on(async {
        let client = LocalClient::connect(&data, &helper, digest).await?;
        client.execute(ExperimentRequest::Stop {}).await?;
        Ok::<_, anyhow::Error>(())
    });
    let exit = host.wait(Duration::from_secs(20));
    result?;
    stopped?;
    ensure!(exit?.success(), "Owned host failed confirmed shutdown.");
    Ok(())
}
