use super::picker::{wrap, Picker};
use anyhow::{bail, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal,
};
use shellfox_ssh::{ProfileDto, Secret};
use std::{
    io::{self, IsTerminal, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
#[error("SSH cancelled")]
pub(super) struct Cancelled;

#[derive(Clone)]
pub(super) struct Terminal {
    pub tty: bool,
    pub color: bool,
    raw: Arc<AtomicBool>,
    prompting: Arc<AtomicBool>,
}
impl Terminal {
    pub fn new() -> Self {
        let stdout = io::stdout().is_terminal();
        Self {
            tty: stdout && io::stdin().is_terminal(),
            color: stdout && std::env::var_os("NO_COLOR").is_none(),
            raw: Arc::new(AtomicBool::new(false)),
            prompting: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn start(&self) -> Result<Option<Raw<'_>>> {
        if !self.tty {
            return Ok(None);
        }
        terminal::enable_raw_mode()?;
        self.raw.store(true, Ordering::Relaxed);
        Ok(Some(Raw(self)))
    }
    pub fn write(&self, text: &str) -> Result<()> {
        let mut output = io::stdout().lock();
        if cfg!(unix) && self.raw.load(Ordering::Relaxed) {
            output.write_all(text.replace('\n', "\r\n").as_bytes())?;
        } else {
            output.write_all(text.as_bytes())?;
        }
        output.flush()?;
        Ok(())
    }
    pub fn line(&self, text: &str) -> Result<()> {
        self.write(&format!("  {text}\n"))
    }
    pub fn style(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }
    pub fn dim(&self, text: &str) -> String {
        self.style("2", text)
    }
    pub(super) fn accent(&self, text: &str) -> String {
        self.style("38;2;236;72;153", text)
    }
    pub fn help(&self, color: bool) -> Result<()> {
        let style = |code, text: &str| {
            if color {
                self.style(code, text)
            } else {
                text.to_owned()
            }
        };
        let help = format!(
            "\n  {} [--profile name]\n  {}\n  {}\n",
            style("38;2;236;72;153", "shellfox ssh"),
            style("2", "connect to a saved ssh connection"),
            style("2", "without a name, choose a connection with ↑↓ and enter")
        );
        let width = io::stdout()
            .is_terminal()
            .then(terminal::size)
            .and_then(Result::ok)
            .map_or(usize::MAX, |(w, _)| usize::from(w));
        self.write(&wrap(&help, width))
    }
    pub async fn prompt(&self, label: &str, hidden: bool) -> Result<Zeroizing<String>> {
        if !self.tty {
            bail!("an interactive terminal is required for login or host trust");
        }
        self.prompting.store(true, Ordering::Relaxed);
        let _prompt = PromptGuard(&self.prompting);
        self.write(&format!("  {label}"))?;
        let mut value = Zeroizing::new(String::new());
        loop {
            match next_event().await? {
                Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                    KeyCode::Enter => {
                        self.write("\n")?;
                        return Ok(value);
                    }
                    KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if matches!(c, 'c' | 'd') {
                            self.write("\n")?;
                            return Err(Cancelled.into());
                        }
                    }
                    KeyCode::Backspace => {
                        if value.pop().is_some() && !hidden {
                            self.write("\x08 \x08")?;
                        }
                    }
                    KeyCode::Char(c) if !c.is_control() => {
                        value.push(c);
                        if !hidden {
                            self.write(c.encode_utf8(&mut [0; 4]))?;
                        }
                    }
                    _ => {}
                },
                Event::Paste(text) => {
                    let text = Zeroizing::new(text);
                    if !text.chars().any(char::is_control) {
                        value.push_str(&text);
                        if !hidden {
                            self.write(&text)?;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    pub async fn secret(&self, label: &str) -> Result<Secret> {
        let mut secret = self.prompt(label, true).await?;
        Ok(Secret::new(std::mem::take(&mut *secret))?)
    }
    pub async fn cancel_before_login(&self) -> Result<()> {
        loop {
            if !self.prompting.load(Ordering::Relaxed) && event::poll(Duration::ZERO)? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Release
                        && key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        return Err(Cancelled.into());
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    pub async fn pick(&self, profiles: &[ProfileDto]) -> Result<usize> {
        let mut screen = Picker {
            terminal: self,
            lines: 0,
        };
        let mut selected = 0;
        let mut size = terminal::size().unwrap_or((120, 24));
        loop {
            screen.draw(profiles, selected, size)?;
            loop {
                match next_event().await? {
                    Event::Resize(cols, rows) => {
                        size = (cols, rows);
                        break;
                    }
                    Event::Key(key) if key.kind != KeyEventKind::Release => {
                        if key.modifiers.contains(KeyModifiers::CONTROL) {
                            if key.code == KeyCode::Char('c') {
                                return Err(Cancelled.into());
                            }
                            continue;
                        }
                        match key.code {
                            KeyCode::Esc | KeyCode::Char('q') => return Err(Cancelled.into()),
                            KeyCode::Up | KeyCode::Char('k') => {
                                selected = (selected + profiles.len() - 1) % profiles.len();
                                break;
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                selected = (selected + 1) % profiles.len();
                                break;
                            }
                            KeyCode::Enter => return Ok(selected),
                            KeyCode::Char(c @ '1'..='9') => {
                                let n = (c as usize) - ('1' as usize);
                                if n < profiles.len() {
                                    return Ok(n);
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
pub(super) async fn next_event() -> io::Result<Event> {
    loop {
        if event::poll(Duration::ZERO)? {
            return event::read();
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
struct PromptGuard<'a>(&'a AtomicBool);
impl Drop for PromptGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}
pub(super) struct Raw<'a>(&'a Terminal);
impl Drop for Raw<'_> {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        self.0.raw.store(false, Ordering::Relaxed);
        let _ = self.0.write("\x1b[0m\x1b[?25h");
    }
}
