//! The 0.9.x `shellfox` command surface (docs/cli.md), translated onto the
//! typed host commands. Existing scripts and service units call it unchanged.
use crate::output::CliError;
use anyhow::{Context, Result};
use std::{ffi::OsString, path::PathBuf};

#[derive(Debug)]
pub struct Invocation {
    /// 0.9.x data root. Rust keeps its own state in `<root>/rust-v1/host`.
    pub user_data: Option<PathBuf>,
    pub json: bool,
    pub yes: bool,
    /// A remote peer ID or hostname; None is this machine.
    pub host: Option<String>,
    pub action: Action,
}

#[derive(Debug, PartialEq)]
pub enum Action {
    /// `help [group]`; None is every group.
    Help(Option<String>),
    Version,
    Update {
        check: bool,
    },
    HostRun,
    HostStatus,
    HostStop,
    /// `host keep-awake [on|off]`; None reads it.
    KeepAwake(Option<bool>),
    Service {
        install: bool,
        startup: bool,
    },
    /// Destructive operations name what is confirmed before running `words`.
    Host {
        words: Vec<String>,
        confirm: Option<String>,
    },
    /// The folder as typed; None is the caller's folder, or `~` on a remote host.
    CreateSession {
        path: Option<String>,
        title: Option<String>,
        request_id: Option<shellfox_core::SessionId>,
    },
    Manager(crate::manager_args::ManagerCommand),
    /// `completion [SHELL]`; None prints the usage.
    Completion(Option<String>),
    /// `terminals attach` on a remote host.
    RemoteAttach(shellfox_core::TabId),
    /// `commands`/`cmd` and everything after it.
    Commands(Vec<String>),
    Machines,
    Approvals,
    /// Not a 0.9.x command. Development hosts selected with --data-dir also
    /// accept the operator commands in args.rs; everything else rejects it.
    Unknown(Vec<String>),
}

const GLOBALS: [&str; 4] = ["--json", "--yes", "--host", "--user-data"];
const VALUED: [&str; 12] = [
    "--tab",
    "--path",
    "--pid",
    "--title",
    "--input",
    "--hostname",
    "--cwd",
    "--profile",
    "--request-id",
    "--auth-key-file",
    "--id",
    "--name",
];

pub fn parse(arguments: &[OsString]) -> Result<Invocation> {
    let mut words = Vec::with_capacity(arguments.len());
    for argument in arguments {
        words.push(
            argument
                .to_str()
                .ok_or_else(|| CliError::usage("Command arguments must be valid UTF-8."))?
                .to_owned(),
        );
    }
    // 0.9.x host terminals run `main.cjs --shellfox-default-user-data DIR ARGS`.
    let mut launcher_default = None;
    if words
        .first()
        .is_some_and(|word| word == "--shellfox-default-user-data")
    {
        let value = words
            .get(1)
            .filter(|v| !v.is_empty() && !v.contains(['\0', '\r', '\n']));
        let value = value
            .cloned()
            .ok_or_else(|| CliError::usage("Invalid launcher user data directory."))?;
        launcher_default = Some(value);
        words.drain(..2);
    }
    let (mut json, mut yes, mut host, mut user_data) = (false, false, None, None);
    let mut rest: Vec<String> = Vec::new();
    // 0.9 parseGlobals: after `cmd NAME` (or `cmd --run NAME`) every word is the script's.
    let mut commands = Commands::No;
    let mut operands = 0;
    let mut index = 0;
    while index < words.len() {
        let word = &words[index];
        if word == "--" {
            rest.extend_from_slice(&words[index..]);
            break;
        }
        if operands > 0 {
            rest.push(word.clone());
            operands -= 1;
            index += 1;
            continue;
        }
        match commands {
            Commands::Operation => {
                if let Some(count) = crate::custom_commands::operand_count(word) {
                    commands = Commands::Admin;
                    operands = count;
                    rest.push(word.clone());
                    index += 1;
                    continue;
                }
                if word == "--run" {
                    commands = Commands::Script;
                    rest.push(word.clone());
                    index += 1;
                    continue;
                }
                if !word.starts_with('-') {
                    rest.extend_from_slice(&words[index..]);
                    break;
                }
            }
            Commands::Script if !word.starts_with('-') => {
                rest.extend_from_slice(&words[index..]);
                break;
            }
            _ => {}
        }
        let (flag, inline) = match word.split_once('=') {
            Some((flag, value)) if word.starts_with("--") => (flag, Some(value.to_owned())),
            _ => (word.as_str(), None),
        };
        if !GLOBALS.contains(&flag) {
            if rest.is_empty() && (word == "cmd" || word == "commands") {
                commands = Commands::Operation;
            }
            rest.push(word.clone());
            if inline.is_none() && VALUED.contains(&flag) {
                if let Some(value) = words.get(index + 1).filter(|v| *v != "--") {
                    rest.push(value.clone());
                    index += 1;
                }
            }
            index += 1;
            continue;
        }
        match flag {
            "--json" | "--yes" if inline.is_some() => {
                return Err(CliError::usage(format!("{flag} does not accept a value.")));
            }
            "--json" | "--yes" => {
                let slot = if flag == "--json" {
                    &mut json
                } else {
                    &mut yes
                };
                if std::mem::replace(slot, true) {
                    return Err(CliError::usage(format!("Duplicate option: {flag}")));
                }
            }
            _ => {
                let value = match inline {
                    Some(value) => value,
                    None => {
                        index += 1;
                        words.get(index).cloned().unwrap_or_default()
                    }
                };
                if value.is_empty() || value.starts_with("--") || value.contains(['\0', '\r', '\n'])
                {
                    return Err(CliError::usage(format!(
                        "Missing or invalid value for {flag}"
                    )));
                }
                let slot = if flag == "--host" {
                    &mut host
                } else {
                    &mut user_data
                };
                if slot.replace(value).is_some() {
                    return Err(CliError::usage(format!("Duplicate option: {flag}")));
                }
            }
        }
        index += 1;
    }
    let host = host.filter(|host| host != "local");
    if host.is_some() {
        local_only(&rest)?;
    }
    let action = action(&rest, json, host.is_some())?;
    // Only these reach a peer; anything else with --host must not quietly run here.
    if host.is_some()
        && !matches!(
            action,
            Action::Manager(_)
                | Action::RemoteAttach(_)
                | Action::CreateSession { .. }
                | Action::Help(_)
                | Action::Version
                | Action::Completion(_)
        )
    {
        return Err(CliError::usage(
            "This command manages the local host. Use --host for session and terminal operations.",
        ));
    }
    Ok(Invocation {
        user_data: user_data_root(user_data.or(launcher_default))?,
        json,
        yes,
        host,
        action,
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Commands {
    No,
    Operation,
    Admin,
    Script,
}

/// The chosen 0.9.x data root; None means the platform default.
fn user_data_root(explicit: Option<String>) -> Result<Option<PathBuf>> {
    let chosen = explicit
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("SHELLFOX_USER_DATA").map(PathBuf::from));
    match chosen {
        Some(path) if path.as_os_str().is_empty() => {
            Err(CliError::usage("Invalid user data directory."))
        }
        Some(path) => Ok(Some(resolve(&std::path::absolute(path)?))),
        None => Ok(None),
    }
}

impl Invocation {
    /// Resolved lazily: an explicit --data-dir needs no platform default.
    pub fn user_data(&self) -> Result<PathBuf> {
        match &self.user_data {
            Some(path) => Ok(path.clone()),
            None => Ok(shellfox_service::app_paths::default_data_root()?
                .ancestors()
                .nth(2)
                .context("Missing Shellfox data root")?
                .to_owned()),
        }
    }
}

/// Node's `path.resolve`: no `.`, `..` or trailing separator, so startup names match 0.9.x.
pub(crate) fn resolve(path: &std::path::Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if out.parent().is_some() {
                    out.pop();
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Rust host state lives beside, never inside, the 0.9.x files.
pub fn data_dir(user_data: &std::path::Path) -> PathBuf {
    user_data.join("rust-v1").join("host")
}

fn words(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}
fn host(items: &[&str]) -> Action {
    Action::Host {
        words: words(items),
        confirm: None,
    }
}
fn confirm(what: String, items: &[&str]) -> Action {
    Action::Host {
        words: words(items),
        confirm: Some(what),
    }
}
fn with_rest(prefix: &[&str], rest: &[&str]) -> Vec<String> {
    words(prefix).into_iter().chain(words(rest)).collect()
}

fn action(args: &[String], json: bool, remote: bool) -> Result<Action> {
    let parts: Vec<&str> = args.iter().map(String::as_str).collect();
    let commands = matches!(parts.first(), Some(&("cmd" | "commands")));
    let trailing_help =
        !commands && parts.len() > 1 && parts.iter().any(|p| *p == "--help" || *p == "-h");
    if let Some(command) = (!trailing_help)
        .then(|| crate::manager_args::parse(&parts))
        .transpose()?
        .flatten()
    {
        return Ok(Action::Manager(command));
    }
    let session = |namespace: &str| matches!(namespace, "session" | "sessions" | "ss");
    let terminal = |namespace: &str| matches!(namespace, "terminals" | "tm");
    Ok(match parts.as_slice() {
        [] | ["help" | "--help" | "-h"] => Action::Help(None),
        ["help", topic, ..] if crate::help::known(topic) => Action::Help(Some((*topic).to_owned())),
        ["help", ..] => return Err(CliError::usage("Unknown help group. Run shellfox help.")),
        ["cmd" | "commands"] => Action::Help(Some("commands".into())),
        [first, ..]
            if trailing_help && *first != "run" && *first != "ssh" && crate::help::known(first) =>
        {
            Action::Help(Some((*first).to_owned()))
        }
        ["version" | "--version" | "-v"] => Action::Version,
        ["update"] => Action::Update { check: false },
        ["update", "--check"] => Action::Update { check: true },
        ["update", ..] => return Err(CliError::usage("Usage: shellfox update [--check] [--json]")),
        // `host serve` is the name development scripts and tests use.
        ["host", "run" | "serve"] => Action::HostRun,
        ["host", "status"] => Action::HostStatus,
        ["host", "stop"] => Action::HostStop,
        ["host", "keep-awake"] => Action::KeepAwake(None),
        ["host", "keep-awake", state @ ("on" | "off")] => Action::KeepAwake(Some(*state == "on")),
        ["host", "service", "install"] => Action::Service {
            install: true,
            startup: false,
        },
        ["host", "service", "install", "--startup"] => Action::Service {
            install: true,
            startup: true,
        },
        ["host", "service", "uninstall"] => Action::Service {
            install: false,
            startup: false,
        },
        [namespace, "create", rest @ ..] if session(namespace) => create_session(rest)?,
        [namespace, "attach", _] if terminal(namespace) && json => {
            return Err(CliError::usage(
                "Interactive attach does not support --json.",
            ));
        }
        [namespace, "attach", tab] if terminal(namespace) && remote => Action::RemoteAttach(
            tab.parse()
                .map_err(|_| CliError::usage("Expected a full session or terminal UUID."))?,
        ),
        [namespace, "attach", tab] if terminal(namespace) => host(&["attach", tab]),
        ["settings", "read"] if remote => {
            Action::Manager(crate::manager_args::ManagerCommand::Settings)
        }
        ["settings", "update", ..] if remote => {
            return Err(CliError::coded(
                "UNSUPPORTED",
                "saveSettings is not available over a remote connection.",
                false,
            ))
        }
        ["settings", "read"] => host(&["settings", "get"]),
        ["settings", "update", "--input", input] => {
            let input = read_input(input)?;
            let object = serde_json::from_str::<serde_json::Value>(&input)
                .map_err(|_| CliError::usage("Invalid JSON input."))?
                .is_object();
            if !object {
                return Err(CliError::usage("Settings input must be a JSON object."));
            }
            host(&["settings", "set", &input])
        }
        ["settings", "update", ..] => return Err(CliError::usage("Provide --input with JSON.")),
        ["network", "status"] => host(&["host", "network", "status"]),
        ["network", "enable"] => host(&["host", "network", "on"]),
        ["network", "enable", "--hostname", name] => {
            host(&["host", "network", "on", "--hostname", name])
        }
        ["network", "disable"] => confirm("network:disable".into(), &["host", "network", "off"]),
        ["network", "login"] => host(&["host", "network", "login"]),
        ["network", "login", "--auth-key-stdin"] => {
            host(&["host", "network", "login", "--auth-key-stdin"])
        }
        ["network", "login", file] if file.starts_with("--auth-key-file=") => host(&[
            "host",
            "network",
            "login",
            "--auth-key-file",
            &file["--auth-key-file=".len()..],
        ]),
        ["network", "login", "--auth-key-file", file] => {
            host(&["host", "network", "login", "--auth-key-file", file])
        }
        ["network", "login", ..]
            if parts.iter().any(|p| p.starts_with("--auth-key-file"))
                && parts.contains(&"--auth-key-stdin") =>
        {
            return Err(CliError::usage(
                "Choose --auth-key-stdin or --auth-key-file.",
            ))
        }
        ["completion"] => Action::Completion(None),
        ["completion", "--help" | "-h" | "help"] => Action::Completion(None),
        ["completion", shell] => Action::Completion(Some((*shell).to_owned())),
        ["completion", ..] => return Err(CliError::usage(crate::completion::HELP.trim())),
        ["cmd" | "commands", rest @ ..] => Action::Commands(words(rest)),
        ["machines", "connect" | "disconnect" | "forget", peer]
        | ["approvals", "approve" | "revoke" | "reject", peer]
            if !valid_peer(peer) =>
        {
            return Err(CliError::usage("Invalid peer ID."))
        }
        ["machines", "list"] => Action::Machines,
        ["machines", op @ ("connect" | "disconnect"), peer] => {
            host(&["host", "network", "peer", peer, op])
        }
        ["machines", "forget", peer] => confirm(
            "machines:forget".into(),
            &["host", "network", "peer", peer, "forget"],
        ),
        ["approvals", "list"] => Action::Approvals,
        ["approvals", "approve", peer] => host(&["host", "network", "peer", peer, "approve"]),
        ["approvals", op @ ("reject" | "revoke"), peer] => confirm(
            format!("approvals:{op}"),
            &["host", "network", "peer", peer, op],
        ),
        ["run", ..] if json && !parts.contains(&"--detach") => {
            return Err(CliError::usage(
                "Foreground run does not support --json. Use run --detach --json.",
            ));
        }
        ["run", rest @ ..] => Action::Host {
            words: with_rest(&["run"], rest),
            confirm: None,
        },
        ["runs"] | ["runs", "list"] => host(&["runs", "list"]),
        ["runs", "show", id] => host(&["runs", "show", id]),
        ["runs", "stop", id] => confirm(format!("Stop run {id}"), &["runs", "stop", id]),
        [namespace, ..] if session(namespace) => {
            return Err(CliError::usage(
                "Unknown session operation. Run shellfox session --help.",
            ))
        }
        [namespace, ..] if terminal(namespace) => {
            return Err(CliError::usage("Unknown terminals operation."))
        }
        ["host", "keep-awake", ..] => {
            return Err(CliError::usage("Usage: shellfox host keep-awake [on|off]"))
        }
        // `host power|network` and `settings get|set` are development commands.
        ["host"] | ["host", "run" | "status" | "stop", ..] => {
            return Err(CliError::usage("Usage: shellfox host run|status|stop"))
        }
        ["settings"] | ["settings", "read", ..] => {
            return Err(CliError::usage(
                "Usage: shellfox settings read|update --input <JSON>",
            ))
        }
        [namespace @ ("network" | "machines" | "approvals"), ..] => {
            return Err(CliError::usage(format!("Unknown {namespace} operation.")))
        }
        _ => Action::Unknown(args.to_vec()),
    })
}

/// 0.9's remotePeerIdSchema.
fn valid_peer(peer: &str) -> bool {
    (1..=200).contains(&peer.len())
        && peer
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
}

/// Flag names with their values, in order.
type Flags<'a> = Vec<(&'a str, &'a str)>;

fn options<'a>(rest: &[&'a str], allowed: &[&str]) -> Result<(Vec<&'a str>, Flags<'a>)> {
    let (mut positional, mut flags) = (Vec::new(), Vec::new());
    let mut items = rest.iter();
    while let Some(item) = items.next() {
        if let Some((flag, value)) = item.split_once('=').filter(|_| item.starts_with("--")) {
            flags.push((flag, value));
        } else if item.starts_with("--") {
            let value = items
                .next()
                .ok_or_else(|| CliError::usage(format!("{item} requires a value.")))?;
            flags.push((*item, *value));
        } else {
            positional.push(*item);
        }
    }
    for (flag, _) in &flags {
        if !allowed.contains(flag) {
            return Err(CliError::usage(format!("Unknown option: {flag}")));
        }
    }
    Ok((positional, flags))
}
fn flag(flags: &[(&str, &str)], name: &str) -> Option<String> {
    flags
        .iter()
        .find(|(flag, _)| *flag == name)
        .map(|(_, value)| (*value).to_owned())
}

#[path = "node_input.rs"]
mod input;
use input::{create_session, local_only};
pub use input::{local_directory, read_input, read_stdin};

#[cfg(test)]
#[path = "node_args_tests.rs"]
mod tests;
