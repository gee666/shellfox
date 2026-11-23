#![cfg(windows)]
use super::test_owned as owned;
use super::*;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
#[ignore = "explicit compiled CLI+verified helper+owned raw fixture only"]
fn actual_fast_natural_status_and_injected_interrupt_preserve_correlated_outcome() -> Result<()> {
    let cli = PathBuf::from(
        std::env::var_os("SHELLFOX_CLI_TEST_EXECUTABLE")
            .context("explicit compiled CLI fixture")?,
    );
    let helper = PathBuf::from(
        std::env::var_os("SHELLFOX_CLI_TEST_HELPER").context("explicit verified helper")?,
    );
    let raw = PathBuf::from(
        std::env::var_os("SHELLFOX_RAW_TEST_HELPER").context("explicit raw fixture")?,
    );
    for path in [&cli, &helper, &raw] {
        if !path.is_absolute() || !path.is_file() {
            bail!("Existing absolute owned fixture paths required.");
        }
    }
    let hash = std::env::var("SHELLFOX_CLI_TEST_HELPER_SHA256")?;
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("Trusted helper digest required.");
    }
    let mut digest = [0; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hash[index * 2..index * 2 + 2], 16)?;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root)?;
    let scratch = tempfile::Builder::new()
        .prefix("cli-control-owner-")
        .tempdir_in(root)?;
    let data = scratch.path().join("data");
    std::fs::create_dir(&data)?;
    let mut host = owned::OwnedHost(
        Command::new(&cli)
            .args([
                "--data-dir",
                data.to_str().context("Unicode fixture")?,
                "--helper",
                helper.to_str().context("Unicode fixture")?,
                "--helper-sha256",
                &hash,
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
            if let Ok(client) = LocalClient::connect(&data, &helper, digest).await {
                break client;
            }
            if Instant::now() >= deadline {
                bail!("Owned endpoint startup deadline.");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        for (status, interrupt) in [
            (7_u32, false),
            (0x8000_0000, false),
            (0xffff_ffff, false),
            (7, true),
        ] {
            let run = client
                .request(&AutomationStart {
                    request_id: Uuid::new_v4(),
                    executable: raw.to_str().context("Unicode raw fixture")?.into(),
                    args: vec!["exit-code".into(), status.to_string()],
                    cwd: scratch.path().to_str().context("Unicode cwd")?.into(),
                    env: BTreeMap::new(),
                    title: None,
                    detached: false,
                })
                .await?;
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if client
                    .request(&AutomationList {})
                    .await?
                    .0
                    .iter()
                    .any(|item| item.id == run.id && item.state == AutomationState::Exited)
                {
                    break;
                }
                if Instant::now() >= deadline {
                    bail!("Owned natural target did not exit.");
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            let (send, input) = mpsc::channel(1);
            send.send(Ok(Vec::new())).await?;
            drop(send);
            let code = if interrupt {
                foreground_with_interrupt(&client, run.id, input, std::future::ready(Ok(())))
                    .await?
            } else {
                foreground_with_interrupt(&client, run.id, input, std::future::pending()).await?
            };
            if code as u32 != status {
                bail!("Fast natural32-bit status was masked by stdin/interrupt handling.");
            }
        }
        let run = client
            .request(&AutomationStart {
                request_id: Uuid::new_v4(),
                executable: raw.to_str().context("Unicode fixture")?.into(),
                args: vec!["idle".into()],
                cwd: scratch.path().to_str().context("Unicode cwd")?.into(),
                env: BTreeMap::new(),
                title: None,
                detached: false,
            })
            .await?;
        let (_send, input) = mpsc::channel(1);
        let code = foreground_with_interrupt(&client, run.id, input, async {
            tokio::time::sleep(Duration::from_millis(30)).await;
            Ok(())
        })
        .await?;
        if code != 137 {
            bail!("Injected interrupt ignored actual confirmed SIGKILL compatibility DTO.");
        }
        let dto = client
            .request(&AutomationList {})
            .await?
            .0
            .into_iter()
            .find(|item| item.id == run.id)
            .context("Actual stopped DTO absent")?;
        if dto.state != AutomationState::Exited || dto.signal.as_deref() != Some("SIGKILL") {
            bail!("Stop convention was not correlated with final owned run.");
        }
        Ok::<_, anyhow::Error>(())
    });
    let stopped = runtime.block_on(async {
        let client = LocalClient::connect(&data, &helper, digest).await?;
        client
            .execute(shellfox_service::local_api::LocalApiRequest::Stop {})
            .await?;
        Ok::<_, anyhow::Error>(())
    });
    let exit = host.wait(Duration::from_secs(20));
    result?;
    stopped?;
    if !exit?.success() {
        bail!("Owned CLI failed shutdown.");
    }
    Ok(())
}
