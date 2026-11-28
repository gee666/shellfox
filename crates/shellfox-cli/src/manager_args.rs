//! 0.9 grammar for the session, terminals, profiles, path and api commands
//! (src/cli/commands.ts, session.ts and main/control-cli.ts).
use crate::output::CliError;
use anyhow::Result;
use serde_json::Value;
use shellfox_core::{SessionId, TabId};

#[derive(Debug, PartialEq)]
pub enum ManagerCommand {
    /// `session list|stats|processes`, read through one fresh snapshot.
    Sessions(Listing, Filters),
    Session(SessionOp, Target),
    Terminals(TerminalOp),
    Profiles,
    Path(PathOp),
    Api {
        method: String,
        payload: Value,
    },
    RemoteApi {
        method: String,
        payload: Value,
    },
    /// `session create`: a resolved local folder, or the path a peer resolves.
    CreateSession {
        cwd: String,
        title: Option<String>,
        request_id: SessionId,
    },
    /// `settings read`: the manager snapshot's settings.
    Settings,
    /// `settings update` on the local host: a validated saveSettings patch.
    SaveSettings(Value),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Listing {
    List,
    Stats,
    Processes,
}
#[derive(Debug, Default, PartialEq)]
pub struct Filters {
    pub running: bool,
    pub idle: bool,
    pub no_agents: bool,
    pub current: bool,
    pub all: bool,
    pub id: Option<SessionId>,
    pub name: Option<String>,
    pub path: Option<String>,
    pub tab: Option<TabId>,
    pub pid: Option<u32>,
}

/// An explicit session UUID, or the terminal this command runs in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Id(SessionId),
    Current,
}

#[derive(Debug, PartialEq)]
pub enum SessionOp {
    Inspect,
    Rename(String),
    Pin(bool),
    Env(Option<Value>),
    Settle { confirm_active: bool },
    Unsettle,
    Delete,
    Focus,
    Refresh,
    ClearError,
}

#[derive(Debug, PartialEq)]
pub enum TerminalOp {
    /// None lists every session's terminals.
    List(Option<Target>),
    Create {
        session: Target,
        title: Option<String>,
        profile: Option<String>,
        cwd: Option<String>,
    },
    Rename {
        session: Target,
        tab: TabId,
        title: String,
    },
    Reorder {
        session: Target,
        tabs: Vec<TabId>,
    },
    Close(TabId),
    Write {
        tab: TabId,
        /// None reads up to 64 KiB from stdin.
        data: Option<String>,
        take: bool,
    },
    /// Output after `--after`, read without taking control.
    Read(shellfox_protocol::ReadTerminal),
}

#[derive(Debug, PartialEq)]
pub enum PathOp {
    Home,
    Resolve(String),
    Complete(String),
    Create(String),
}

/// Values and booleans after the operation, split like 0.9's CommandArguments.
struct Arguments {
    positionals: Vec<String>,
    values: Vec<(String, Option<String>)>,
}
impl Arguments {
    fn parse(args: &[&str], valued: &[&str], booleans: &[&str]) -> Result<Self> {
        let mut parsed = Self {
            positionals: Vec::new(),
            values: Vec::new(),
        };
        let mut index = 0;
        while index < args.len() {
            let arg = args[index];
            index += 1;
            if arg == "--" {
                parsed
                    .positionals
                    .extend(args[index..].iter().map(|a| (*a).to_owned()));
                break;
            }
            if !arg.starts_with("--") {
                parsed.positionals.push(arg.to_owned());
                continue;
            }
            let (flag, inline) = match arg.split_once('=') {
                Some((flag, value)) => (flag, Some(value.to_owned())),
                None => (arg, None),
            };
            if parsed.has(flag) {
                return Err(CliError::usage(format!("Duplicate option: {flag}")));
            }
            if booleans.contains(&flag) {
                if inline.is_some() {
                    return Err(CliError::usage(format!("{flag} does not accept a value.")));
                }
                parsed.values.push((flag.to_owned(), None));
            } else if valued.contains(&flag) {
                let value = match inline {
                    Some(value) => value,
                    None => {
                        index += 1;
                        args.get(index - 1)
                            .filter(|value| **value != "--")
                            .map(|value| (*value).to_owned())
                            .ok_or_else(|| CliError::usage(format!("Missing value for {flag}")))?
                    }
                };
                parsed.values.push((flag.to_owned(), Some(value)));
            } else {
                return Err(CliError::usage(format!("Unknown option: {flag}")));
            }
        }
        Ok(parsed)
    }
    fn has(&self, flag: &str) -> bool {
        self.values.iter().any(|(name, _)| name == flag)
    }
    fn text(&self, flag: &str) -> Option<String> {
        self.values
            .iter()
            .find(|(name, _)| name == flag)
            .and_then(|(_, value)| value.clone())
    }
    fn arity(&self, min: usize, max: usize) -> Result<&[String]> {
        let count = self.positionals.len();
        if count < min || count > max {
            let expected = if min == max {
                min.to_string()
            } else {
                format!("{min} to {max}")
            };
            return Err(CliError::usage(format!(
                "Expected {expected} positional arguments."
            )));
        }
        Ok(&self.positionals)
    }
}

fn uuid<T: std::str::FromStr>(value: &str, message: &str) -> Result<T> {
    let full = value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        });
    full.then(|| value.to_ascii_lowercase().parse().ok())
        .flatten()
        .ok_or_else(|| CliError::usage(message))
}
/// One explicit session UUID, else the current terminal's session.
fn session_target(positional: Option<&str>, flag: Option<String>) -> Result<Target> {
    if positional.is_some() && flag.is_some() {
        return Err(CliError::usage(
            "Choose one explicit session ID, not both a positional ID and --id.",
        ));
    }
    match positional.map(str::to_owned).or(flag) {
        Some(id) => Ok(Target::Id(uuid(&id, "Expected a full session UUID.")?)),
        None => Ok(Target::Current),
    }
}
fn title(value: &str, message: &str) -> Result<String> {
    shellfox_core::validation::title(value).map_err(|_| CliError::usage(message))?;
    Ok(value.to_owned())
}
fn json(text: &str) -> Result<Value> {
    serde_json::from_str(&crate::node_args::read_input(text)?)
        .map_err(|_| CliError::usage("Invalid JSON input."))
}

pub fn parse(words: &[&str]) -> Result<Option<ManagerCommand>> {
    let (namespace, operation, rest) = match words {
        [namespace, operation, rest @ ..] => (*namespace, *operation, rest),
        [namespace] => (*namespace, "", &[][..]),
        [] => return Ok(None),
    };
    // A bare group name prints that group's help.
    if operation.is_empty()
        && matches!(
            namespace,
            "session" | "sessions" | "ss" | "terminals" | "tm"
        )
    {
        return Ok(None);
    }
    Ok(Some(match namespace {
        "session" | "sessions" | "ss" => match operation {
            "list" => ManagerCommand::Sessions(Listing::List, filters(Listing::List, rest)?),
            "stats" => ManagerCommand::Sessions(Listing::Stats, filters(Listing::Stats, rest)?),
            "processes" => {
                ManagerCommand::Sessions(Listing::Processes, filters(Listing::Processes, rest)?)
            }
            "create" => return Ok(None),
            operation => session(operation, rest)?,
        },
        "terminals" | "tm" if operation == "attach" => return Ok(None),
        "terminals" | "tm" => ManagerCommand::Terminals(terminals(operation, rest)?),
        "profiles" => {
            if !matches!(operation, "" | "list") || !rest.is_empty() {
                return Err(CliError::usage("Usage: shellfox profiles [list]"));
            }
            ManagerCommand::Profiles
        }
        "path" => ManagerCommand::Path(path(operation, rest)?),
        "api" => {
            if operation.is_empty() {
                return Err(CliError::usage("Unknown ManagerMethod."));
            }
            // Validate before connecting, as 0.9 did: `api nope` fails with no host.
            let payload = api_input(rest)?;
            crate::manager::parse(operation, payload.clone())?;
            ManagerCommand::Api {
                method: operation.to_owned(),
                payload,
            }
        }
        "remote" if operation == "api" => {
            let (method, rest) = rest
                .split_first()
                .ok_or_else(|| CliError::usage("Unknown remote method."))?;
            let payload = api_input(rest)?;
            if *method == "request" {
                remote_request(&payload)?;
            }
            ManagerCommand::RemoteApi {
                method: (*method).to_owned(),
                payload,
            }
        }
        _ => return Ok(None),
    }))
}

fn api_input(rest: &[&str]) -> Result<Value> {
    let args = Arguments::parse(rest, &["--input"], &[])?;
    args.arity(0, 0)?;
    args.text("--input")
        .map_or(Ok(Value::Object(Default::default())), |input| json(&input))
}

#[path = "manager_args_more.rs"]
mod more;
use more::{filters, path, remote_request, session, terminals};

#[cfg(test)]
#[path = "manager_args_tests.rs"]
mod tests;
