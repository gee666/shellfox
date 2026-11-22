//! The 0.9.x `shellfox` command surface (docs/cli.md), translated onto the
//! typed host commands. Existing scripts and service units call it unchanged.
use crate::output::CliError;
use anyhow::{Context, Result};
use std::{ffi::OsString, path::PathBuf};

#[derive(Debug)]
pub struct Invocation {
    /// 0.9.x data root. Rust keeps its own state in `<root>/rust-v1/host`.
    pub user_data: PathBuf,
    pub json: bool,
    pub yes: bool,
    pub action: Action,
}

#[derive(Debug, PartialEq)]
pub enum Action {
    Help,
    Version,
    Update {
        check: bool,
    },
    HostRun,
    HostStatus,
    HostStop,
    Service {
        install: bool,
        startup: bool,
    },
    /// Destructive operations name what is confirmed before running `words`.
    Host {
        words: Vec<String>,
        confirm: Option<String>,
    },
    CreateSession {
        cwd: String,
        title: Option<String>,
    },
    CreateTab {
        session: String,
        title: Option<String>,
        profile: Option<String>,
    },
    Inspect(String),
    Machines,
    Approvals,
}

const GLOBALS: [&str; 4] = ["--json", "--yes", "--host", "--user-data"];
const VALUED: [&str; 9] = [
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
    let (mut json, mut yes, mut host, mut user_data) = (false, false, None, None);
    let mut rest = Vec::new();
    let mut index = 0;
    while index < words.len() {
        let word = &words[index];
        if word == "--" {
            rest.extend_from_slice(&words[index..]);
            break;
        }
        let (flag, inline) = match word.split_once('=') {
            Some((flag, value)) if word.starts_with("--") => (flag, Some(value.to_owned())),
            _ => (word.as_str(), None),
        };
        if !GLOBALS.contains(&flag) {
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
            "--json" => json = true,
            "--yes" => yes = true,
            _ => {
                let value = match inline {
                    Some(value) => value,
                    None => {
                        index += 1;
                        words
                            .get(index)
                            .cloned()
                            .ok_or_else(|| CliError::usage(format!("{flag} requires a value.")))?
                    }
                };
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
    if host.as_deref().is_some_and(|host| host != "local") {
        return Err(CliError::new(
            "UNSUPPORTED",
            "Remote --host targets are not available in this CLI yet. Run the command on that machine.",
            false,
        ));
    }
    let action = action(&rest, json)?;
    Ok(Invocation {
        user_data: user_data_root(user_data)?,
        json,
        yes,
        action,
    })
}

fn user_data_root(explicit: Option<String>) -> Result<PathBuf> {
    let chosen = explicit
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("SHELLFOX_USER_DATA").map(PathBuf::from));
    match chosen {
        Some(path) if path.as_os_str().is_empty() => {
            Err(CliError::usage("Invalid user data directory."))
        }
        Some(path) => Ok(std::path::absolute(path)?),
        None => Ok(shellfox_service::app_paths::default_data_root()?
            .ancestors()
            .nth(2)
            .context("Missing Shellfox data root")?
            .to_owned()),
    }
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

fn action(args: &[String], json: bool) -> Result<Action> {
    let parts: Vec<&str> = args.iter().map(String::as_str).collect();
    let trailing_help = parts.len() > 1 && parts.iter().any(|p| *p == "--help" || *p == "-h");
    let session = |namespace: &str| matches!(namespace, "session" | "sessions" | "ss");
    let terminal = |namespace: &str| matches!(namespace, "terminals" | "tm");
    Ok(match parts.as_slice() {
        [] | ["help" | "--help" | "-h", ..] => Action::Help,
        _ if trailing_help && parts[0] != "run" => Action::Help,
        ["version" | "--version" | "-v"] => Action::Version,
        ["update"] => Action::Update { check: false },
        ["update", "--check"] => Action::Update { check: true },
        ["update", ..] => return Err(CliError::usage("Usage: shellfox update [--check] [--json]")),
        ["host", "run"] => Action::HostRun,
        ["host", "status"] => Action::HostStatus,
        ["host", "stop"] => Action::HostStop,
        ["host", "keep-awake"] => host(&["host", "power", "status"]),
        ["host", "keep-awake", state @ ("on" | "off")] => host(&["host", "power", state]),
        ["host", "service", "install"] => Action::Service { install: true, startup: false },
        ["host", "service", "install", "--startup"] => Action::Service { install: true, startup: true },
        ["host", "service", "uninstall"] => Action::Service { install: false, startup: false },
        [namespace, "list"] if session(namespace) => host(&["session", "list"]),
        [namespace, "create", rest @ ..] if session(namespace) => create_session(rest)?,
        [namespace, "inspect", id] if session(namespace) => Action::Inspect((*id).into()),
        [namespace, "rename", id, title] if session(namespace) => host(&["session", "rename", id, title]),
        [namespace, "delete" | "rm" | "-rm", id] if session(namespace) => {
            confirm(format!("session delete {id}"), &["session", "close", id])
        }
        [namespace, "list"] if terminal(namespace) => host(&["tab", "list"]),
        [namespace, "list", id] if terminal(namespace) => host(&["tab", "list", id]),
        [namespace, "create", session, rest @ ..] if terminal(namespace) => create_tab(session, rest)?,
        [namespace, "rename", _session, tab, title] if terminal(namespace) => host(&["tab", "rename", tab, title]),
        [namespace, "close", tab] if terminal(namespace) => confirm(format!("Close terminal {tab}"), &["tab", "close", tab]),
        [namespace, "attach", _] if terminal(namespace) && json => {
            return Err(CliError::usage("Interactive attach does not support --json."));
        }
        [namespace, "attach", tab] if terminal(namespace) => host(&["attach", tab]),
        ["settings", "read"] => host(&["settings", "get"]),
        ["settings", "update", "--input", input] => host(&["settings", "set", &read_input(input)?]),
        ["network", "status"] => host(&["host", "network", "status"]),
        ["network", "enable"] => host(&["host", "network", "on"]),
        ["network", "enable", "--hostname", name] => host(&["host", "network", "on", "--hostname", name]),
        ["network", "disable"] => confirm("network:disable".into(), &["host", "network", "off"]),
        ["network", "login"] => host(&["host", "network", "login"]),
        ["network", "login", "--auth-key-stdin"] => host(&["host", "network", "login", "--auth-key-stdin"]),
        ["machines", "list"] => Action::Machines,
        ["machines", op @ ("connect" | "disconnect"), peer] => host(&["host", "network", "peer", peer, op]),
        ["machines", "forget", peer] => confirm("machines:forget".into(), &["host", "network", "peer", peer, "forget"]),
        ["approvals", "list"] => Action::Approvals,
        ["approvals", "approve", peer] => host(&["host", "network", "peer", peer, "approve"]),
        ["approvals", op @ ("reject" | "revoke"), peer] => {
            confirm(format!("approvals:{op}"), &["host", "network", "peer", peer, op])
        }
        ["run", ..] if json && !parts.contains(&"--detach") => {
            return Err(CliError::usage("Foreground run does not support --json. Use run --detach --json."));
        }
        ["run", rest @ ..] => Action::Host { words: with_rest(&["run"], rest), confirm: None },
        ["runs"] | ["runs", "list"] => host(&["runs", "list"]),
        ["runs", "show", id] => host(&["runs", "show", id]),
        ["runs", "stop", id] => confirm(format!("Stop run {id}"), &["runs", "stop", id]),
        _ => return Err(CliError::usage(
            "Unknown or unsupported command. Run shellfox help for the commands this version supports.",
        )),
    })
}

fn options<'a>(
    rest: &[&'a str],
    allowed: &[&str],
) -> Result<(Vec<&'a str>, Vec<(&'a str, &'a str)>)> {
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

fn create_session(rest: &[&str]) -> Result<Action> {
    let (positional, flags) = options(rest, &["--title"])?;
    let path = match positional.as_slice() {
        [] => ".",
        [path] => path,
        _ => {
            return Err(CliError::usage(
                "Usage: shellfox session create [PATH] [--title TITLE]",
            ))
        }
    };
    let cwd = std::path::absolute(path)
        .ok()
        .filter(|cwd| cwd.is_dir())
        .ok_or_else(|| CliError::usage("Invalid directory."))?;
    let cwd = crate::headless_layout::normalize(&cwd)
        .into_os_string()
        .into_string()
        .map_err(|_| CliError::usage("Invalid directory."))?;
    Ok(Action::CreateSession {
        cwd,
        title: flag(&flags, "--title"),
    })
}

fn create_tab(session: &str, rest: &[&str]) -> Result<Action> {
    let (positional, flags) = options(rest, &["--title", "--profile"])?;
    if !positional.is_empty() {
        return Err(CliError::usage(
            "Usage: shellfox terminals create SESSION_UUID [--title TITLE] [--profile PROFILE_ID]",
        ));
    }
    Ok(Action::CreateTab {
        session: session.into(),
        title: flag(&flags, "--title"),
        profile: flag(&flags, "--profile"),
    })
}

/// `--input` accepts inline JSON, `@file.json` or `-` for stdin, up to 1 MiB.
fn read_input(input: &str) -> Result<String> {
    use std::io::Read;
    const LIMIT: u64 = 1024 * 1024;
    let mut text = String::new();
    if input == "-" {
        std::io::stdin().take(LIMIT + 1).read_to_string(&mut text)?;
    } else if let Some(file) = input.strip_prefix('@') {
        std::fs::File::open(file)
            .with_context(|| format!("Could not read {file}"))?
            .take(LIMIT + 1)
            .read_to_string(&mut text)?;
    } else {
        text = input.to_owned();
    }
    if text.len() as u64 > LIMIT {
        return Err(CliError::usage("JSON input exceeds 1 MiB."));
    }
    Ok(text)
}

#[cfg(test)]
#[path = "node_args_tests.rs"]
mod tests;
