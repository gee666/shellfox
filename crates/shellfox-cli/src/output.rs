//! One output policy per process: the experimental raw JSON, or the 0.9.x CLI
//! contract (`{"ok":true,"value":...}` with `--json`, readable text otherwise).
use serde::Serialize;
use std::{
    fmt,
    io::{self, Write},
    sync::OnceLock,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Raw,
    Json,
    Human,
}
static MODE: OnceLock<Mode> = OnceLock::new();

pub fn set_mode(mode: Mode) {
    let _ = MODE.set(mode);
}
pub fn mode() -> Mode {
    *MODE.get().unwrap_or(&Mode::Raw)
}

/// A failure with the stable code and exit status that 0.9.x scripts check.
#[derive(Debug)]
pub struct CliError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}
impl CliError {
    pub fn usage(message: impl Into<String>) -> anyhow::Error {
        Self::new("VALIDATION", message, false)
    }
    pub fn new(code: &'static str, message: impl Into<String>, retryable: bool) -> anyhow::Error {
        Self {
            code,
            message: message.into(),
            retryable,
        }
        .into()
    }
}
impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for CliError {}

pub fn print_value(value: &impl Serialize) -> anyhow::Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    match mode() {
        Mode::Json => {
            let line = serde_json::to_string(&serde_json::json!({ "ok": true, "value": value }))?;
            writeln!(output, "{line}")?;
        }
        Mode::Raw | Mode::Human => {
            let value = serde_json::to_value(value)?;
            let text = match &value {
                serde_json::Value::String(text) if mode() == Mode::Human => text.clone(),
                value => serde_json::to_string_pretty(value)?,
            };
            writeln!(output, "{}", crate::terminal_text::escape_json_display(&text))?;
        }
    }
    Ok(())
}

/// Returns the process exit code: 2 for invalid arguments, 1 for failures.
pub fn print_error(error: &anyhow::Error) -> i32 {
    let known = error.downcast_ref::<CliError>();
    let code = known.map_or("INTERNAL", |error| error.code);
    let message = known.map_or_else(|| format!("{error:#}"), |error| error.message.clone());
    match mode() {
        Mode::Json => {
            let record = serde_json::json!({ "ok": false, "error": {
                "code": code, "message": message, "retryable": known.is_some_and(|e| e.retryable),
            }});
            println!("{record}");
        }
        Mode::Human => eprintln!(
            "Shellfox: {}",
            crate::terminal_text::escape_json_display(&message)
        ),
        Mode::Raw => eprintln!("shellfox-cli: {error:#}"),
    }
    if code == "VALIDATION" {
        2
    } else {
        1
    }
}
