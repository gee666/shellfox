//! Shared TTY admission and cleanup for local and native-peer attach.
use crate::presentation;
use anyhow::{Result, bail};
use crossterm::{event::EnableBracketedPaste, execute, terminal};
use std::io::{self, IsTerminal, Write};
pub fn require_terminal() -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("attach requires terminal stdin and stdout; redirected output is not supported.");
    }
    Ok(())
}

pub(crate) struct TerminalGuard {
    was_raw: bool,
}
impl TerminalGuard {
    pub(crate) fn enter() -> Result<Self> {
        let was_raw = terminal::is_raw_mode_enabled()?;
        terminal::enable_raw_mode()?;
        let guard = Self { was_raw };
        io::stdout().write_all(presentation::ENTER)?;
        execute!(io::stdout(), EnableBracketedPaste)?;
        Ok(guard)
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut output = io::stdout().lock();
        let _ = output.write_all(presentation::RESET);
        let _ = output.write_all(presentation::LEAVE);
        let _ = output.flush();
        if !self.was_raw {
            let _ = terminal::disable_raw_mode();
        }
    }
}
