use crate::support::OwnedHost;
use anyhow::{ensure, Context, Result};
use shellfox_service::local::{automation::*, LocalClient};
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
fn command(data: &Path, helper: &Path, hash: &str, cwd: &Path) -> Result<Command> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_shellfox-cli"));
    command
        .env_clear()
        .env(
            "SystemRoot",
            std::env::var_os("SystemRoot").context("Windows system root")?,
        )
        .env("USERPROFILE", cwd)
        .env("TEMP", cwd)
        .env("TMP", cwd)
        .current_dir(cwd)
        .args([
            "--data-dir",
            data.to_str().context("Unicode fixture")?,
            "--helper",
            helper.to_str().context("Unicode helper")?,
            "--helper-sha256",
            hash,
        ]);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(command)
}
pub async fn actual_cli_lifetime(
    client: &LocalClient,
    data: &Path,
    helper: &Path,
    hash: &str,
    raw: &Path,
    cwd: &Path,
) -> Result<()> {
    for code in [7_u32, 0xffff_ffff] {
        let mut command = command(data, helper, hash, cwd)?;
        command.args([
            "run",
            "--",
            raw.to_str().context("Unicode raw fixture")?,
            "exit-code",
            &code.to_string(),
        ]);
        let mut child = OwnedHost(command.spawn()?);
        let held = child.0.stdin.take().context("Held-open caller stdin")?;
        let status = child.wait(Duration::from_secs(12))?;
        drop(held); // Deliberately AFTER actual CLI exit; no EOF was supplied.
        ensure!(
            status.code().map(|code| code as u32) == Some(code),
            "Held-open stdin masked/hung the actual natural32-bit CLI exit."
        );
    }
    let mut command = command(data, helper, hash, cwd)?;
    command.args([
        "run",
        "--title",
        "owned abrupt EOF fixture",
        "--",
        raw.to_str().context("Unicode raw fixture")?,
        "eof",
    ]);
    let mut child = OwnedHost(command.spawn()?);
    let held = child.0.stdin.take().context("Held caller stdin")?;
    let deadline = Instant::now() + Duration::from_secs(15);
    let id = loop {
        let list = client.request(&AutomationList {}).await?;
        if let Some(run) = list
            .0
            .iter()
            .find(|run| run.title == "owned abrupt EOF fixture")
        {
            ensure!(
                run.state == AutomationState::Running,
                "Raw target exited before owned client disappearance."
            );
            break run.id;
        }
        ensure!(
            Instant::now() < deadline,
            "Owned client never started its target."
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    child.0.kill()?;
    child.wait(Duration::from_secs(5))?;
    drop(held); // Original owned CLI handle only.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let list = client.request(&AutomationList {}).await?;
        let run = list
            .0
            .iter()
            .find(|run| run.id == id)
            .context("Owned EOF target record vanished")?;
        if run.state == AutomationState::Exited {
            ensure!(
                run.exit_code == Some(0) && run.signal.is_none(),
                "Client loss killed raw target instead of delivering EOF."
            );
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "Actual client death did not close stdin before30s lease expiry."
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Ok(())
}
