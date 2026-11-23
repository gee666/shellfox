use anyhow::{Context, Result, bail, ensure};
use shellfox_ghostty_vt::key::{KeyEvent, KeyModifiers, PhysicalKey};
use shellfox_ghostty_vt::Terminal;
use shellfox_terminal::{PtyConfig, PtyOutput, PtySession};
use std::{
    path::Path,
    time::{Duration, Instant},
};

/// Starts only an owned experiment shell and closes its Job before returning.
pub fn run(run_dir: &Path) -> Result<()> {
    let mut config = PtyConfig::user(run_dir);
    config.profile = Some(if cfg!(windows) {
        shellfox_terminal::ShellProfileId::Cmd
    } else {
        shellfox_terminal::ShellProfileId::LoginShell
    });
    // The same console supervisor the host uses for every shell.
    config.supervisor = Some(crate::host_process::cli_path()?);
    let mut pty = PtySession::spawn(config).map_err(|error| anyhow::anyhow!("{error}"))?;
    let result = (|| -> Result<()> {
        let mut terminal = Terminal::new(80, 24)?;
        let command = terminal.encode_paste("echo SHELLFOX_PTY_SMOKE_OK")?;
        pty.write(&command)?;
        let enter =
            terminal.encode_key_event(&KeyEvent::press(PhysicalKey::Enter, KeyModifiers::NONE))?;
        pty.write(&enter)?;
        pty.resize(100, 30)?;
        terminal.resize(100, 30)?;
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut found = false;
        while Instant::now() < deadline {
            match pty.try_read() {
                Ok(Some(PtyOutput::Data(bytes))) => {
                    terminal.feed(&bytes)?;
                    let replies = terminal.drain_replies()?;
                    if !replies.is_empty() {
                        pty.write(&replies)?;
                    }
                    let screen = terminal.render()?;
                    let text: String = screen.cells.iter().map(|cell| cell.text.as_str()).collect();
                    // Echoed command contains the marker too, so require a full output row.
                    found = screen.cells.chunks(usize::from(screen.cols)).any(|row| {
                        row.iter()
                            .map(|cell| cell.text.as_str())
                            .collect::<String>()
                            .trim()
                            == "SHELLFOX_PTY_SMOKE_OK"
                    });
                    if found {
                        ensure!(text.contains("SHELLFOX_PTY_SMOKE_OK"));
                        break;
                    }
                }
                Ok(Some(PtyOutput::ReadError(error))) => return Err(error.into()),
                Ok(Some(PtyOutput::Eof)) => bail!("PTY exited before marker"),
                Err(error) => bail!("PTY output disconnected: {error}"),
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        ensure!(found, "PTY smoke timed out without parsed shell output");
        Ok(())
    })();
    pty.shutdown().context("smoke cleanup failed")?;
    result?;
    println!("PASS: owned PTY shell output -> Ghostty screen, encoded input, resize, cleanup");
    Ok(())
}
