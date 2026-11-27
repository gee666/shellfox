//! Real shells behind ConPTY, driven by Ghostty key events through the host.
//! Each case types into a live shell and checks what the shell then printed.
#![cfg(windows)]
#[path = "../../test_dev_helper.rs"]
mod dev_helper;
mod support;
use support::OwnedHost;

use anyhow::{bail, ensure, Context, Result};
use shellfox_core::{CreateSession, ProfileId, SessionId, TabId};
use shellfox_ghostty_vt::key::{KeyEvent, KeyModifiers as M, PhysicalKey as P};
use shellfox_ghostty_vt::Terminal;
use shellfox_service::{
    local::LocalClient,
    local_api::{LocalApiRequest as Request, LocalApiResponse as Response},
    Attachment,
};
use std::{
    os::windows::process::CommandExt,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const COLS: u16 = 160;
const ROWS: u16 = 40;

#[test]
fn conpty_shells_edit_lines_with_ghostty_keys() -> Result<()> {
    let (helper, digest) = dev_helper::helper()?;
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .context("no workspace root")?;
    std::fs::create_dir_all(root.join("tmp"))?;
    let scratch = tempfile::Builder::new()
        .prefix("cli-keys-conpty-")
        .tempdir_in(root.join("tmp"))?;
    let data = scratch.path().join("data");
    std::fs::create_dir(&data)?;
    // The desktop starts its host in a new process group, which hands every
    // tab the "ignore Ctrl+C" flag unless the supervisor clears it.
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    let mut host = OwnedHost(
        Command::new(env!("CARGO_BIN_EXE_shellfox-cli"))
            .creation_flags(CREATE_NEW_PROCESS_GROUP)
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
    let result = runtime.block_on(async {
        let client = connect(&mut host, &data, &helper, digest).await?;
        let outcome = shells(&client, scratch.path()).await;
        let _ = client.execute(Request::Stop {}).await;
        outcome
    });
    let exit = host.wait(Duration::from_secs(20));
    result?;
    ensure!(exit?.success(), "CLI host exited unsuccessfully.");
    Ok(())
}

async fn shells(client: &LocalClient, cwd: &Path) -> Result<()> {
    cmd(&mut Shell::open(client, cwd, "cmd").await.context("open cmd")?)
        .await
        .context("cmd.exe")?;
    eprintln!("cmd.exe passed");
    let pwsh_present = which("pwsh.exe");
    for profile in ["windows-powershell", "pwsh"] {
        if profile == "pwsh" && !pwsh_present {
            eprintln!("pwsh.exe not installed; skipped");
            continue;
        }
        let mut shell = Shell::open(client, cwd, profile)
            .await
            .with_context(|| format!("open {profile}"))?;
        psreadline(&mut shell).await.context(profile)?;
        if profile == "windows-powershell" {
            tui(&mut shell).await.context("pi-style TUI")?;
        }
        eprintln!("{profile} passed");
    }
    // WSL runs the user's own login files (which may prompt for sudo), so it
    // only runs on request: SHELLFOX_TEST_WSL=1.
    let wsl = std::env::var_os("SHELLFOX_TEST_WSL").is_some_and(|v| v == "1");
    if let Some(distribution) = (wsl && which("wsl.exe")).then(wsl_distribution).flatten() {
        let profile = format!("wsl:{distribution}");
        bash(
            &mut Shell::open(client, cwd, &profile)
                .await
                .context("open wsl")?,
        )
        .await
        .context("WSL bash")?;
        eprintln!("WSL bash passed");
    } else {
        eprintln!("WSL skipped");
    }
    Ok(())
}

async fn cmd(s: &mut Shell<'_>) -> Result<()> {
    s.wait_prompt(">").await?;
    s.text("echo SFC1x").await?;
    s.key(P::Backspace, M::NONE).await?;
    s.key(P::Enter, M::NONE).await?;
    s.wait_row("SFC1").await?;
    s.text("echo SFAC").await?;
    s.key(P::ArrowLeft, M::NONE).await?;
    s.text("B").await?;
    s.key(P::Enter, M::NONE).await?;
    s.wait_row("SFABC").await?;
    // Long enough that only an interrupt brings the prompt back in time.
    s.text("ping -n 120 127.0.0.1").await?;
    s.key(P::Enter, M::NONE).await?;
    s.wait_contains("127.0.0.1").await?;
    s.ctrl('c').await?;
    s.text("echo SFCC").await?;
    s.key(P::Enter, M::NONE).await?;
    s.wait_row("SFCC").await
}

async fn psreadline(s: &mut Shell<'_>) -> Result<()> {
    s.wait_prompt("> ").await?;
    // Ctrl+Backspace deletes the word before the cursor.
    s.text("Write-Host SFK1 SFK2junk").await?;
    s.key(P::Backspace, M::CONTROL).await?;
    s.text("SFK3").await?;
    s.key(P::Enter, M::NONE).await?;
    s.wait_row("SFK1 SFK3").await?;
    // Ctrl+Left moves one word; Home and End jump.
    s.text("Host SFA SFC").await?;
    s.key(P::ArrowLeft, M::CONTROL).await?;
    s.text("SFB ").await?;
    s.key(P::Home, M::NONE).await?;
    s.text("Write-").await?;
    s.key(P::End, M::NONE).await?;
    s.key(P::Enter, M::NONE).await?;
    s.wait_row("SFA SFB SFC").await?;
    // Shift+Left selects; typing replaces the selection.
    s.text("Write-Host SFX junk").await?;
    for _ in 0..4 {
        s.key(P::ArrowLeft, M::SHIFT).await?;
    }
    s.text("SFY").await?;
    s.key(P::Enter, M::NONE).await?;
    s.wait_row("SFX SFY").await?;
    // Ctrl+Shift+Left selects a word.
    s.text("Write-Host SFW junkword").await?;
    s.key(P::ArrowLeft, M::CONTROL | M::SHIFT).await?;
    s.key(P::Delete, M::NONE).await?;
    s.text("SFV").await?;
    s.key(P::Enter, M::NONE).await?;
    s.wait_row("SFW SFV").await?;
    // Up recalls history.
    s.key(P::ArrowUp, M::NONE).await?;
    s.key(P::Enter, M::NONE).await?;
    s.wait_rows("SFW SFV", 2).await?;
    // Ctrl+C interrupts a running program on any layout.
    s.text("ping -n 120 127.0.0.1").await?;
    s.key(P::Enter, M::NONE).await?;
    s.wait_contains("127.0.0.1:").await?;
    s.ctrl('c').await?;
    s.text("Write-Host SFINT").await?;
    s.key(P::Enter, M::NONE).await?;
    s.wait_row("SFINT").await?;
    // Ctrl+C abandons the line. Shellfox sends ETX on any layout and ConPTY
    // turns it into a Ctrl+C key record, but PSReadLine maps that record
    // through the shell's keyboard layout. With a layout that has no Latin C
    // active when the shell starts (Russian, Greek...), it types that layout's
    // letter instead, in any terminal.
    if latin_layout() {
        s.text("Write-Host SFBAD").await?;
        s.ctrl('c').await?;
        s.text("Write-Host SFOK").await?;
        s.key(P::Enter, M::NONE).await?;
        s.wait_row("SFOK").await?;
        ensure!(!s.has_row("SFBAD")?, "Ctrl+C did not cancel the line");
    } else {
        eprintln!("active keyboard layout has no Latin C; PSReadLine Ctrl+C skipped");
    }
    // A multi-line paste runs each line.
    s.paste("Write-Host SFP1\r\nWrite-Host SFP2\r\n").await?;
    s.wait_row("SFP2").await?;
    ensure!(s.has_row("SFP1")?, "first pasted line did not run");
    Ok(())
}

/// pi-tui's two renderers under ConPTY. Main screen: rewritten bottom lines
/// that scroll into history. Full screen: the alternate screen with mouse
/// reporting, so the wheel reaches the app. The inbox ConPTY dropped the
/// mouse modes, and the wheel became Up/Down, recalling pi's prompt history.
async fn tui(s: &mut Shell<'_>) -> Result<()> {
    let frames = concat!(
        "$e=[char]27; 1..200 | % { [Console]::Write(\"${e}[?2026h`r${e}[2Kmessage $_",
        "`r`n${e}[2Ksecond line`r`n${e}[2Kinput>${e}[?2026l\") }; Write-Host ''; Write-Host SFTUI\r\n"
    );
    s.paste(frames).await?;
    s.wait_row("SFTUI").await?;
    let history = s.screen.recovery_info()?.scrollback_rows;
    ensure!(history >= 350, "only {history} rows reached scrollback");
    let full = concat!(
        "$e=[char]27; [Console]::Write(\"${e}[?1049h${e}[?1000h${e}[?1006h\"); Start-Sleep 3; ",
        "[Console]::Write(\"${e}[?1006l${e}[?1000l${e}[?1049l\"); Write-Host SFALT\r\n"
    );
    s.paste(full).await?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while !(s.screen.recovery_info()?.alternate && s.screen.mouse_tracking()?) {
        ensure!(
            Instant::now() < deadline,
            "the alternate screen never got mouse reporting"
        );
        s.pump().await?;
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    ensure!(
        !s.screen.alternate_scroll()?,
        "the wheel would send cursor keys"
    );
    s.wait_row("SFALT").await?;
    ensure!(
        !s.screen.mouse_tracking()?,
        "mouse reporting outlived the app"
    );
    Ok(())
}

async fn bash(s: &mut Shell<'_>) -> Result<()> {
    s.wait_prompt("$ ").await?;
    s.text("echo SFB1x").await?;
    s.key(P::Backspace, M::NONE).await?;
    s.key(P::Enter, M::NONE).await?;
    s.wait_row("SFB1").await?;
    // Readline: Ctrl+A to line start, Alt+F one word forward.
    s.text("cho SFB2 SFB4").await?;
    s.ctrl('a').await?;
    s.text("e").await?;
    s.chord('f', M::ALT).await?;
    s.chord('f', M::ALT).await?;
    s.text(" SFB3").await?;
    s.key(P::Enter, M::NONE).await?;
    s.wait_row("SFB2 SFB3 SFB4").await?;
    s.text("sleep 30").await?;
    s.key(P::Enter, M::NONE).await?;
    s.ctrl('c').await?;
    s.text("echo SFB5").await?;
    s.key(P::Enter, M::NONE).await?;
    s.wait_row("SFB5").await?;
    s.text("printf 'SF%s\\n' \"$((6*7))\"").await?;
    s.key(P::Enter, M::NONE).await?;
    s.wait_row("SF42").await
}

struct Shell<'a> {
    client: &'a LocalClient,
    tab: TabId,
    attachment: Attachment,
    sequence: u64,
    screen: Terminal,
}

impl<'a> Shell<'a> {
    async fn open(client: &'a LocalClient, cwd: &Path, profile: &str) -> Result<Shell<'a>> {
        let snapshot = match client
            .execute(Request::CreateSession {
                input: CreateSession {
                    request_id: SessionId::new(),
                    title: Some(format!("keys {profile}")),
                    cwd: cwd.to_str().context("UTF-8 path")?.into(),
                    profile_id: ProfileId::new(profile)?,
                },
            })
            .await?
        {
            Response::Snapshot(snapshot) => snapshot,
            _ => bail!("Invalid create response."),
        };
        let tab = snapshot.selection.tab_id.context("no tab")?;
        let attachment = match client
            .execute(Request::Attach {
                tab,
                takeover: false,
            })
            .await?
        {
            Response::Attachment(value) => value,
            _ => bail!("Invalid attachment response."),
        };
        let resize = Request::Resize {
            tab,
            attachment: attachment.clone(),
            cols: COLS,
            rows: ROWS,
        };
        done(client.execute(resize).await?)?;
        let mut shell = Shell {
            client,
            tab,
            attachment,
            sequence: 0,
            screen: Terminal::new(COLS, ROWS)?,
        };
        match client
            .execute(Request::Recover {
                tab,
                attachment: shell.attachment.clone(),
            })
            .await?
        {
            Response::Recovery {
                bytes,
                last_sequence,
                ..
            } => {
                shell.screen.feed(&bytes)?;
                shell.sequence = last_sequence;
            }
            _ => bail!("Invalid recovery response."),
        }
        Ok(shell)
    }
    async fn send(&self, event: KeyEvent) -> Result<()> {
        let request = Request::Key {
            tab: self.tab,
            attachment: self.attachment.clone(),
            event,
        };
        done(self.client.execute(request).await?)?;
        // Shells read input as it arrives; pace keys like a fast typist.
        tokio::time::sleep(Duration::from_millis(15)).await;
        Ok(())
    }
    async fn key(&self, physical: P, mods: M) -> Result<()> {
        let event = KeyEvent::press(physical, mods);
        self.send(event.clone()).await?;
        self.send(event.release()).await
    }
    async fn ctrl(&self, c: char) -> Result<()> {
        self.chord(c, M::CONTROL).await
    }
    /// Modifier+letter as the desktop sends it: the letter's key and its text.
    async fn chord(&self, c: char, mods: M) -> Result<()> {
        let physical = P::try_from(P::KeyA.code() + u32::from(c) - u32::from('a'))?;
        let event = KeyEvent {
            text: c.into(),
            unshifted_codepoint: Some(c),
            ..KeyEvent::press(physical, mods)
        };
        self.send(event.clone()).await?;
        self.send(event.release()).await
    }
    /// Typed text arrives as committed text, one character at a time.
    async fn text(&self, text: &str) -> Result<()> {
        for c in text.chars() {
            self.send(KeyEvent::text(c)).await?;
        }
        Ok(())
    }
    async fn paste(&self, text: &str) -> Result<()> {
        let request = Request::Paste {
            tab: self.tab,
            attachment: self.attachment.clone(),
            text: text.into(),
        };
        done(self.client.execute(request).await?)
    }
    async fn pump(&mut self) -> Result<()> {
        let chunks = match self
            .client
            .execute(Request::Poll {
                tab: self.tab,
                attachment: self.attachment.clone(),
            })
            .await?
        {
            Response::Output(chunks) => chunks,
            Response::TerminalExited { .. } => bail!("shell exited"),
            other => bail!("Invalid output response: {other:?}"),
        };
        for chunk in chunks {
            if chunk.sequence <= self.sequence {
                continue;
            }
            ensure!(chunk.sequence == self.sequence + 1, "output gap");
            self.screen.feed(&chunk.bytes)?;
            self.screen.drain_replies()?;
            self.sequence = chunk.sequence;
        }
        let ack = Request::Acknowledge {
            tab: self.tab,
            attachment: self.attachment.clone(),
            sequence: self.sequence,
        };
        done(self.client.execute(ack).await?)
    }
    fn rows(&mut self) -> Result<Vec<String>> {
        let screen = self.screen.render()?;
        Ok(screen
            .cells
            .chunks(usize::from(screen.cols))
            .map(|row| {
                let line: String = row
                    .iter()
                    .map(|c| {
                        if c.text.is_empty() {
                            " "
                        } else {
                            c.text.as_str()
                        }
                    })
                    .collect();
                line.trim().to_owned()
            })
            .collect())
    }
    fn has_row(&mut self, text: &str) -> Result<bool> {
        Ok(self.rows()?.iter().any(|row| row == text))
    }
    async fn wait(&mut self, what: &str, mut done: impl FnMut(&[String]) -> bool) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            self.pump().await?;
            let rows = self.rows()?;
            if done(&rows) {
                return Ok(());
            }
            if Instant::now() > deadline {
                bail!("timed out waiting for {what}; screen:\n{}", rows.join("\n"));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    async fn wait_row(&mut self, text: &str) -> Result<()> {
        self.wait_rows(text, 1).await
    }
    async fn wait_rows(&mut self, text: &str, count: usize) -> Result<()> {
        self.wait(text, |rows| {
            rows.iter().filter(|row| *row == text).count() >= count
        })
        .await
    }
    async fn wait_contains(&mut self, text: &str) -> Result<()> {
        self.wait(text, |rows| {
            rows.iter().filter(|row| row.contains(text)).count() >= 2
        })
        .await
    }
    async fn wait_prompt(&mut self, suffix: &str) -> Result<()> {
        let suffix = suffix.trim_end();
        self.wait("prompt", |rows| {
            rows.iter()
                .any(|row| !row.is_empty() && row.ends_with(suffix))
        })
        .await
    }
}

async fn connect(
    host: &mut OwnedHost,
    data: &Path,
    helper: &Path,
    digest: [u8; 32],
) -> Result<LocalClient> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let endpoint = data.join("rust-service-v1/endpoint.json");
    loop {
        ensure!(
            host.0.try_wait()?.is_none(),
            "CLI host exited before publishing its endpoint."
        );
        if endpoint.is_file() {
            match LocalClient::connect(data, helper, digest).await {
                Ok(client) => return Ok(client),
                Err(error) if Instant::now() >= deadline => {
                    return Err(error.context("host startup"))
                }
                Err(_) => {}
            }
        }
        ensure!(Instant::now() < deadline, "host startup deadline");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn latin_layout() -> bool {
    #[link(name = "user32")]
    extern "system" {
        fn VkKeyScanW(ch: u16) -> i16;
    }
    // SAFETY: a plain lookup in this thread's current keyboard layout.
    unsafe { VkKeyScanW(u16::from(b'c')) != -1 }
}

fn which(exe: &str) -> bool {
    Command::new("where.exe")
        .arg(exe)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The first installed distribution. `wsl -l -q` prints UTF-16.
fn wsl_distribution() -> Option<String> {
    let out = Command::new("wsl.exe").args(["-l", "-q"]).output().ok()?;
    let units: Vec<u16> = out
        .stdout
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    let text = String::from_utf16_lossy(&units);
    let name = text
        .lines()
        .map(|line| line.trim_matches(|c: char| c.is_whitespace() || c == '\0'))
        .find(|line| !line.is_empty())?;
    out.status.success().then(|| name.to_owned())
}

fn done(response: Response) -> Result<()> {
    ensure!(
        matches!(response, Response::Done {}),
        "Invalid command response: {response:?}"
    );
    Ok(())
}
