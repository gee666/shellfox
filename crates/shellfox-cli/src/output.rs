//! Output policy: the 0.9.x CLI contract (`{"ok":true,"value":...}` with
//! `--json`, readable text otherwise), or raw JSON for development operator commands.
use serde::Serialize;
use std::{
    borrow::Cow,
    fmt,
    io::{self, Write},
    sync::atomic::{AtomicU8, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Raw,
    Json,
    Human,
}
static MODE: AtomicU8 = AtomicU8::new(Mode::Raw as u8);

pub fn set_mode(mode: Mode) {
    MODE.store(mode as u8, Ordering::Relaxed);
}
pub fn mode() -> Mode {
    match MODE.load(Ordering::Relaxed) {
        1 => Mode::Json,
        2 => Mode::Human,
        _ => Mode::Raw,
    }
}

/// A failure with the stable code and exit status that 0.9.x scripts check.
#[derive(Debug)]
pub struct CliError {
    pub code: Cow<'static, str>,
    pub message: String,
    pub retryable: bool,
    /// Invalid arguments exit 2; every other failure, host VALIDATION included, exits 1.
    usage: bool,
}
impl CliError {
    pub fn usage(message: impl Into<String>) -> anyhow::Error {
        Self {
            code: Cow::Borrowed("VALIDATION"),
            message: message.into(),
            retryable: false,
            usage: true,
        }
        .into()
    }
    pub fn new(code: &'static str, message: impl Into<String>, retryable: bool) -> anyhow::Error {
        Self {
            code: Cow::Borrowed(code),
            message: message.into(),
            retryable,
            usage: false,
        }
        .into()
    }
    /// A host or peer failure keeps its wire code, as 0.9 printed it.
    pub fn app(error: shellfox_protocol::AppError) -> anyhow::Error {
        Self::from_app(error).into()
    }
    fn from_app(error: shellfox_protocol::AppError) -> Self {
        let code = serde_json::to_value(error.code)
            .ok()
            .and_then(|code| code.as_str().map(str::to_owned))
            .unwrap_or_else(|| "INTERNAL".into());
        Self {
            code: Cow::Owned(code),
            message: error.message,
            retryable: error.retryable,
            usage: false,
        }
    }
}
impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for CliError {}

/// 0.9 records, `ok` first. A struct fixes the field order whatever serde_json map features other crates enable.
#[derive(Serialize)]
#[serde(untagged)]
enum Record<'a, T: Serialize> {
    Ok { ok: bool, value: &'a T },
    Error { ok: bool, error: Failure<'a> },
}
#[derive(Serialize)]
struct Failure<'a> {
    code: &'a str,
    message: &'a str,
    retryable: bool,
}

pub fn print_value(value: &impl Serialize) -> anyhow::Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    match mode() {
        Mode::Json => {
            let line = serde_json::to_string(&Record::Ok { ok: true, value })?;
            writeln!(output, "{line}")?;
        }
        Mode::Human => {
            let value = serde_json::to_value(value)?;
            let style = crate::human_text::Style(crate::human_text::color(true));
            let width = crate::human_text::terminal_width();
            writeln!(output, "{}", crate::human::render(&value, style, width))?;
        }
        Mode::Raw => {
            let text = serde_json::to_string_pretty(value)?;
            writeln!(
                output,
                "{}",
                crate::terminal_text::escape_json_display(&text)
            )?;
        }
    }
    Ok(())
}

/// Trusted text such as help or a completion hook: verbatim, or a JSON string value.
pub fn print_text(text: &str) -> anyhow::Result<()> {
    if mode() == Mode::Json {
        return print_value(&text);
    }
    let stdout = io::stdout();
    let mut output = stdout.lock();
    output.write_all(text.as_bytes())?;
    Ok(output.flush()?)
}

/// Returns the process exit code: 2 for invalid arguments, 1 for failures.
pub fn print_error(error: &anyhow::Error) -> i32 {
    let host = error
        .downcast_ref::<shellfox_service::local::LocalRequestFailure>()
        .map(|failure| CliError::from_app(failure.error.clone()));
    let known = error.downcast_ref::<CliError>().or(host.as_ref());
    let code = known.map_or("INTERNAL", |error| &error.code);
    let message = known.map_or_else(|| format!("{error:#}"), |error| error.message.clone());
    match mode() {
        Mode::Json => {
            let error = Failure { code, message: &message, retryable: known.is_some_and(|e| e.retryable) };
            let record = Record::<()>::Error { ok: false, error };
            println!("{}", serde_json::to_string(&record).unwrap_or_default());
        }
        Mode::Human => eprintln!(
            "{}{}",
            crate::human_text::Style(crate::human_text::color(false)).red("Shellfox: "),
            crate::human_text::escape(&message)
        ),
        Mode::Raw => eprintln!("shellfox-cli: {error:#}"),
    }
    if known.is_some_and(|error| error.usage) {
        2
    } else {
        1
    }
}
