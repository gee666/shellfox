//! `shellfox cmd buil<Tab>` typed into a real host tab completes a command
//! registered with the real CLI, then runs it: PowerShell on Windows, bash
//! on Linux and macOS. The registry lives in scratch data.
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
use std::{
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

#[test]
fn tab_completes_registered_command_names() -> Result<()> {
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

fn quote(word: &Path) -> String {
    let word = word.display().to_string();
    if cfg!(windows) {
        format!("'{}'", word.replace('\'', "''"))
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// A `shellfox` on the tab's PATH that runs this CLI, and a registered
/// `buildall` script that writes `ran`.
fn prepare(root: &Path) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    let (bin, user_data) = (root.join("bin"), root.join("user-data"));
    std::fs::create_dir(&bin)?;
    std::fs::create_dir(&user_data)?;
    let cli = Path::new(env!("CARGO_BIN_EXE_shellfox-cli"));
    let ran = root.join("ran");
    let script = if cfg!(windows) {
        // A shim like the installed one; the CLI needs the DLLs beside it.
        std::fs::write(
            bin.join("shellfox.cmd"),
            format!(
                "@\"{}\" %*
",
                cli.display()
            ),
        )?;
        let script = root.join("buildall.ps1");
        std::fs::write(
            &script,
            format!("Set-Content -LiteralPath {} -Value ok\n", quote(&ran)),
        )?;
        script
    } else {
        #[cfg(unix)]
        std::os::unix::fs::symlink(cli, bin.join("shellfox"))?;
        let script = root.join("buildall.sh");
        std::fs::write(&script, format!("echo ok > {}\n", quote(&ran)))?;
        script
    };
    for (name, path) in [("buildall", &script), ("lint", &script)] {
        let status = Command::new(cli)
            .args(["cmd", "--register", name])
            .arg(path)
            .env("SHELLFOX_USER_DATA", &user_data)
            .status()?;
        ensure!(status.success(), "register {name}");
    }
    Ok((bin, user_data))
}

async fn run(host: &Fixture, helper: &Path, digest: [u8; 32]) -> Result<()> {
    let root = host.scratch.path();
    let (bin, user_data) = prepare(root)?;
    let session = host.json(&["session", "create", "--title", "Completion", "--json"])?;
    let id = session["id"].as_str().context("id")?.to_owned();
    let profiles = host.json(&["profiles", "list", "--json"])?;
    let available = |id: &str| {
        profiles["profiles"]
            .as_array()
            .is_some_and(|all| all.iter().any(|p| p["id"] == id && p["available"] == true))
    };
    let profile = if cfg!(windows) {
        if available("pwsh") {
            "pwsh"
        } else {
            "windows-powershell"
        }
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
    let client = LocalClient::connect(&host.data, helper, digest).await?;
    let attachment = match client
        .execute(LocalApiRequest::Attach {
            tab: tab.parse()?,
            takeover: false,
        })
        .await?
    {
        LocalApiResponse::Attachment(attachment) => attachment,
        _ => bail!("Unexpected attach response."),
    };
    let ready = root.join("ready");
    let setup = if cfg!(windows) {
        format!(
            "$env:PATH = {} + ';' + $env:PATH; $env:SHELLFOX_USER_DATA = {}; Set-Content -LiteralPath {} -Value ok\r",
            quote(&bin),
            quote(&user_data),
            quote(&ready)
        )
    } else {
        format!(
            "export PATH={}:\"$PATH\" SHELLFOX_USER_DATA={}; echo ok > {}\r",
            quote(&bin),
            quote(&user_data),
            quote(&ready)
        )
    };
    let input = |bytes: &[u8]| LocalApiRequest::Input {
        tab: tab.parse().expect("tab id"),
        attachment: attachment.clone(),
        bytes: bytes.to_vec(),
    };
    let poll = || LocalApiRequest::Poll {
        tab: tab.parse().expect("tab id"),
        attachment: attachment.clone(),
    };
    let mut screen = Vec::new();
    client.execute(input(setup.as_bytes())).await?;
    wait(&client, &poll, &mut screen, &ready, "The tab setup").await?;
    // The shell's own Tab handling completes the name; Enter then runs it.
    client.execute(input(b"shellfox cmd buil\t")).await?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    client.execute(input(b"\r")).await?;
    wait(
        &client,
        &poll,
        &mut screen,
        &root.join("ran"),
        "Running the completed command",
    )
    .await?;
    let _ = client.disconnect().await;
    Ok(())
}

/// Waits for `path` while draining the tab's output into `screen`.
async fn wait(
    client: &LocalClient,
    poll: &dyn Fn() -> LocalApiRequest,
    screen: &mut Vec<u8>,
    path: &Path,
    what: &str,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !path.exists() {
        if let Ok(LocalApiResponse::Output(chunks)) = client.execute(poll()).await {
            chunks
                .into_iter()
                .for_each(|chunk| screen.extend(chunk.bytes));
        }
        ensure!(
            Instant::now() < deadline,
            "{what} did not happen. Screen:
{}",
            String::from_utf8_lossy(screen)
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Ok(())
}
