//! The desktop host process outlives its UI and survives the loss of one shell.
#![cfg(any(windows, target_os = "macos"))]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
mod support;
use support::OwnedHost;

use anyhow::{bail, ensure, Context, Result};
use shellfox_core::{CreateSession, ProfileId, SessionId, TabId};
use shellfox_service::{
    experiment::{ExperimentRequest as Request, ExperimentResponse as Response},
    local::{HostClosed, LocalBootstrap, LocalClient},
    Attachment, TerminalOutput,
};
use std::{
    ffi::OsString,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// A stand-in UI that only records that the host relaunched it.
fn fake_ui(marker: &Path) -> (PathBuf, Vec<OsString>) {
    if cfg!(windows) {
        let system = std::env::var_os("SystemRoot").expect("SystemRoot");
        let shell = PathBuf::from(system).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        // The host appends --owned-host; the trailing comment swallows it.
        let script = format!("Set-Content -LiteralPath '{}' -Value relaunched; #", marker.display());
        (shell, vec!["-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), script.into()])
    } else {
        let script = format!("echo relaunched > '{}'", marker.display());
        ("/bin/sh".into(), vec!["-c".into(), script.into(), "sh".into()])
    }
}

struct Fixture {
    scratch: tempfile::TempDir,
    data: PathBuf,
    helper: PathBuf,
    digest: [u8; 32],
    marker: PathBuf,
    host: OwnedHost,
    ui_pipe: Option<std::process::ChildStdin>,
}
impl Fixture {
    fn start() -> Result<Self> {
        let (helper, digest) = dev_helper::helper()?;
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).context("no workspace")?;
        std::fs::create_dir_all(root.join("tmp"))?;
        let scratch = tempfile::Builder::new().prefix("cli-desktop-host-").tempdir_in(root.join("tmp"))?;
        let data = scratch.path().join("data");
        std::fs::create_dir(&data)?;
        let marker = scratch.path().join("relaunched.txt");
        let (ui, relaunch) = fake_ui(&marker);
        let mut host = OwnedHost(
            Command::new(env!("CARGO_BIN_EXE_shellfox-cli"))
                .arg(LocalBootstrap::DESKTOP_HOST_FLAG)
                .arg(&ui)
                .arg(relaunch.len().to_string())
                .args(&relaunch)
                .arg("--data-dir")
                .arg(&data)
                .arg("--helper")
                .arg(&helper)
                .arg("--helper-sha256")
                .arg(hex)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()?,
        );
        let mut ready = String::new();
        BufReader::new(host.0.stdout.take().context("stdout")?).read_line(&mut ready)?;
        ensure!(ready.trim_end() == LocalBootstrap::DESKTOP_HOST_READY, "host did not start: {ready}");
        let ui_pipe = host.0.stdin.take();
        Ok(Self { scratch, data, helper, digest, marker, host, ui_pipe })
    }
    async fn client(&self) -> Result<LocalClient> {
        LocalClient::connect(&self.data, &self.helper, self.digest).await
    }
    /// Stops the host even after a failed assertion, then reports both.
    fn finish(mut self, runtime: &tokio::runtime::Runtime, result: Result<()>) -> Result<()> {
        if result.is_err() {
            runtime.block_on(async {
                if let Ok(client) = self.client().await {
                    let _ = client.execute(Request::Stop {}).await;
                }
            });
        }
        let exit = self.host.wait(Duration::from_secs(30));
        result?;
        ensure!(exit?.success(), "Desktop host exited unsuccessfully.");
        Ok(())
    }
}

#[test]
fn shells_survive_a_ui_crash_and_the_kill_of_another_shell() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let mut fixture = Fixture::start()?;
    let result = runtime.block_on(async {
        let client = fixture.client().await?;
        let cwd = fixture.scratch.path();
        let doomed = create(&client, cwd).await?;
        let survivor = create(&client, cwd).await?;
        let mut doomed = Terminal::attach(&client, doomed).await?;
        let mut survivor = Terminal::attach(&client, survivor).await?;
        kill(doomed.shell_pid(&client).await?)?;
        survivor.echo(&client).await?;
        // The UI dies without its quit byte: the host relaunches it once.
        drop(fixture.ui_pipe.take());
        let deadline = Instant::now() + Duration::from_secs(30);
        while !fixture.marker.is_file() {
            ensure!(Instant::now() < deadline, "The host did not relaunch its UI.");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        survivor.echo(&client).await?;
        stop(&client).await
    });
    fixture.finish(&runtime, result)
}

#[test]
fn a_ui_that_quits_is_not_relaunched() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let mut fixture = Fixture::start()?;
    let result = runtime.block_on(async {
        let mut pipe = fixture.ui_pipe.take().context("stdin")?;
        pipe.write_all(&[LocalBootstrap::DESKTOP_HOST_QUIT])?;
        drop(pipe);
        tokio::time::sleep(Duration::from_secs(2)).await;
        ensure!(!fixture.marker.exists(), "A quitting UI was relaunched.");
        let watcher = fixture.client().await?;
        stop(&fixture.client().await?).await?;
        ensure!(watcher.closed().await == HostClosed::Stopped, "An orderly stop looked like a crash.");
        Ok(())
    });
    fixture.finish(&runtime, result)
}

#[test]
fn a_killed_host_is_told_apart_from_a_stopped_one() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let mut fixture = Fixture::start()?;
    let closed = runtime.block_on(async {
        let watcher = fixture.client().await?;
        fixture.host.0.kill()?;
        anyhow::Ok(watcher.closed().await)
    })?;
    fixture.host.0.wait()?;
    ensure!(closed == HostClosed::Lost, "A killed host looked like an orderly stop.");
    Ok(())
}

async fn stop(client: &LocalClient) -> Result<()> {
    match client.execute(Request::Stop {}).await? {
        Response::Done {} => Ok(()),
        _ => bail!("Invalid stop response."),
    }
}
async fn create(client: &LocalClient, cwd: &Path) -> Result<TabId> {
    let input = CreateSession {
        request_id: SessionId::new(),
        title: Some("desktop host test".into()),
        cwd: cwd.to_str().context("UTF-8 path")?.into(),
        profile_id: ProfileId::new(if cfg!(windows) { "cmd" } else { "login-shell" })?,
    };
    match client.execute(Request::CreateSession { input }).await? {
        Response::Snapshot(snapshot) => snapshot.selection.tab_id.context("no selected tab"),
        _ => bail!("Invalid create response."),
    }
}

struct Terminal {
    tab: TabId,
    attachment: Attachment,
    sequence: u64,
}
impl Terminal {
    async fn attach(client: &LocalClient, tab: TabId) -> Result<Self> {
        let Response::Attachment(attachment) = client.execute(Request::Attach { tab, takeover: false }).await? else {
            bail!("Invalid attachment response.")
        };
        let mut terminal = Self { tab, attachment, sequence: 0 };
        let recover = Request::Recover { tab, attachment: terminal.attachment.clone() };
        let Response::Recovery { last_sequence, .. } = client.execute(recover).await? else {
            bail!("Invalid recovery response.")
        };
        terminal.acknowledge(client, last_sequence).await?;
        Ok(terminal)
    }
    async fn acknowledge(&mut self, client: &LocalClient, sequence: u64) -> Result<()> {
        let ack = Request::Acknowledge { tab: self.tab, attachment: self.attachment.clone(), sequence };
        ensure!(matches!(client.execute(ack).await?, Response::Done {}), "Invalid ack response.");
        self.sequence = sequence;
        Ok(())
    }
    /// Types `command` and returns the output between `start` and `end`.
    async fn run(&mut self, client: &LocalClient, command: &str, start: &str, end: &str) -> Result<String> {
        let bytes = format!("{command}\r").into_bytes();
        let input = Request::Input { tab: self.tab, attachment: self.attachment.clone(), bytes };
        ensure!(matches!(client.execute(input).await?, Response::Done {}), "Invalid input response.");
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut text = String::new();
        loop {
            let poll = Request::Poll { tab: self.tab, attachment: self.attachment.clone() };
            let Response::Output(chunks) = client.execute(poll).await? else { bail!("Invalid poll response.") };
            let mut last = self.sequence;
            for chunk in chunks.iter().filter(|chunk: &&TerminalOutput| chunk.sequence > self.sequence) {
                text.push_str(&String::from_utf8_lossy(&chunk.bytes));
                last = chunk.sequence;
            }
            if last > self.sequence {
                self.acknowledge(client, last).await?;
            }
            if let Some((value, _)) = text.split_once(start).and_then(|(_, after)| after.split_once(end)) {
                return Ok(value.to_owned());
            }
            ensure!(Instant::now() < deadline, "No {start}..{end} output in: {text:?}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    async fn shell_pid(&mut self, client: &LocalClient) -> Result<u32> {
        // Split markers so the echoed command itself never matches.
        let command = if cfg!(windows) {
            r#"powershell -NoProfile -Command "'SHELL' + 'PID=' + (Get-CimInstance Win32_Process -Filter \"ProcessId=$PID\").ParentProcessId + '=E' + 'ND'""#
        } else {
            r#"printf 'SHELL%s%s=E%s\n' PID= $$ ND"#
        };
        let pid = self.run(client, command, "SHELLPID=", "=END").await?;
        Ok(pid.trim().parse()?)
    }
    async fn echo(&mut self, client: &LocalClient) -> Result<()> {
        let command = if cfg!(windows) { "echo ALIVE^=MARK^=END" } else { "printf 'ALIVE%s\\n' =MARK=END" };
        self.run(client, command, "ALIVE=MARK", "=END").await.map(drop)
    }
}

fn kill(pid: u32) -> Result<()> {
    let status = if cfg!(windows) {
        Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).stdout(Stdio::null()).status()?
    } else {
        Command::new("kill").args(["-KILL", &pid.to_string()]).status()?
    };
    ensure!(status.success(), "Could not kill shell {pid}.");
    Ok(())
}
