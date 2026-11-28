//! The session operations, filters for `session list|stats|processes`, the
//! terminals and path grammar, and the `remote api request` check.
use super::*;

pub(super) fn session(operation: &str, rest: &[&str]) -> Result<ManagerCommand> {
    let operation = match operation {
        "rm" | "-rm" => "delete",
        operation => operation,
    };
    let valued: &[&str] = if operation == "env" {
        &["--id", "--input"]
    } else {
        &["--id"]
    };
    let booleans: &[&str] = match operation {
        "pin" => &["--current", "--off"],
        "settle" => &["--current", "--confirm-active"],
        _ => &["--current"],
    };
    let args = Arguments::parse(rest, valued, booleans)?;
    if operation == "rename" {
        let positional = args.arity(1, 2)?;
        if positional.len() == 1
            && !args.has("--id")
            && uuid::<SessionId>(&positional[0], "").is_ok()
        {
            return Err(CliError::usage(
                "Missing the new title after the session UUID.",
            ));
        }
        let id = (positional.len() == 2).then(|| positional[0].as_str());
        let target = session_target(id, args.text("--id"))?;
        let name = title(&positional[positional.len() - 1], "Invalid session title.")?;
        return Ok(ManagerCommand::Session(SessionOp::Rename(name), target));
    }
    let positional = args.arity(0, 1)?;
    let target = session_target(positional.first().map(String::as_str), args.text("--id"))?;
    let op = match operation {
        "inspect" => SessionOp::Inspect,
        "unsettle" => SessionOp::Unsettle,
        "delete" => SessionOp::Delete,
        "focus" => SessionOp::Focus,
        "refresh" => SessionOp::Refresh,
        "clear-error" => SessionOp::ClearError,
        "pin" => SessionOp::Pin(!args.has("--off")),
        "env" => SessionOp::Env(args.text("--input").map(|text| json(&text)).transpose()?),
        "settle" => SessionOp::Settle {
            confirm_active: args.has("--confirm-active"),
        },
        _ => {
            return Err(CliError::usage(
                "Unknown session operation. Run shellfox session --help.",
            ))
        }
    };
    Ok(ManagerCommand::Session(op, target))
}

pub(super) fn filters(listing: Listing, rest: &[&str]) -> Result<Filters> {
    let mut filters = Filters::default();
    let mut seen: Vec<&str> = Vec::new();
    let mut index = 0;
    while index < rest.len() {
        let (flag, inline) = match rest[index].split_once('=') {
            Some((flag, value)) => (flag, Some(value)),
            None => (rest[index], None),
        };
        index += 1;
        if !flag.starts_with('-') {
            if filters.id.is_some() {
                return Err(CliError::usage("Expected one full session UUID."));
            }
            filters.id = Some(uuid(flag, "Expected one full session UUID.")?);
            continue;
        }
        if seen.contains(&flag) {
            return Err(CliError::usage(format!("Duplicate option: {flag}")));
        }
        seen.push(flag);
        let boolean = match flag {
            "--running" => Some(&mut filters.running),
            "--idle" => Some(&mut filters.idle),
            "--no-agents" => Some(&mut filters.no_agents),
            "--current" => Some(&mut filters.current),
            "--all" => Some(&mut filters.all),
            _ => None,
        };
        if let Some(slot) = boolean {
            if inline.is_some() {
                return Err(CliError::usage(format!("Invalid option: {flag}")));
            }
            *slot = true;
            continue;
        }
        if !matches!(flag, "--id" | "--name" | "--path" | "--tab" | "--pid")
            || (flag == "--pid" && listing != Listing::Processes)
        {
            return Err(CliError::usage(format!("Invalid option: {flag}")));
        }
        let value = match inline {
            Some(value) => Some(value),
            None => {
                index += 1;
                rest.get(index - 1).copied()
            }
        }
        .filter(|value| !value.is_empty() && *value != "--")
        .ok_or_else(|| CliError::usage(format!("Missing or duplicate value for {flag}")))?;
        match flag {
            "--id" if filters.id.is_none() => {
                filters.id = Some(uuid(value, "--id requires a full UUID")?);
            }
            "--tab" => filters.tab = Some(uuid(value, "--tab requires a full UUID")?),
            "--pid" => {
                filters.pid = Some(
                    value
                        .parse()
                        .ok()
                        .filter(|pid| *pid > 0 && !value.starts_with('0'))
                        .ok_or_else(|| CliError::usage("--pid requires a positive integer"))?,
                );
            }
            "--name" => filters.name = Some(value.to_owned()),
            "--path" => filters.path = Some(value.to_owned()),
            _ => {
                return Err(CliError::usage(format!(
                    "Missing or duplicate value for {flag}"
                )))
            }
        }
    }
    if filters.running && filters.idle {
        return Err(CliError::usage("Do not combine --running and --idle"));
    }
    if filters.all && (filters.id.is_some() || filters.current) {
        return Err(CliError::usage("Choose --all or one session target."));
    }
    if filters.id.is_some() {
        filters.current = false;
    } else if !filters.all && listing != Listing::List {
        filters.current = true;
    }
    Ok(filters)
}

pub(super) fn terminals(operation: &str, rest: &[&str]) -> Result<TerminalOp> {
    let (valued, booleans): (&[&str], &[&str]) = match operation {
        "create" => (&["--title", "--profile", "--cwd", "--id"], &["--current"]),
        "rename" | "list" | "reorder" => (&["--id"], &["--current"]),
        "write" => (&[], &["--stdin", "--take"]),
        "read" => (&["--after", "--generation"], &[]),
        _ => (&[], &[]),
    };
    let args = Arguments::parse(rest, valued, booleans)?;
    let tab = |value: &str| uuid::<TabId>(value, "Expected a full session or terminal UUID.");
    let first = args.positionals.first().map(String::as_str);
    if let Some(first) = first {
        tab(first)?;
    }
    Ok(match operation {
        "list" => {
            args.arity(0, 1)?;
            let explicit = first.is_some() || args.has("--id");
            (explicit || args.has("--current"))
                .then(|| session_target(first, args.text("--id")))
                .transpose()?
                .map_or(TerminalOp::List(None), |target| {
                    TerminalOp::List(Some(target))
                })
        }
        "create" => {
            args.arity(0, 1)?;
            TerminalOp::Create {
                session: session_target(first, args.text("--id"))?,
                title: args.text("--title"),
                profile: args.text("--profile").filter(|p| !p.is_empty()),
                cwd: args.text("--cwd").filter(|c| !c.is_empty()),
            }
        }
        "rename" => {
            let positional = args.arity(2, 3)?;
            let with_session = positional.len() == 3;
            let session = session_target(
                with_session.then(|| positional[0].as_str()),
                args.text("--id"),
            )?;
            let (tab_id, name) = if with_session {
                (&positional[1], &positional[2])
            } else {
                (&positional[0], &positional[1])
            };
            TerminalOp::Rename {
                session,
                tab: tab(tab_id)?,
                title: title(name, "Invalid terminal title.")?,
            }
        }
        "reorder" => {
            let contextual =
                args.has("--current") || args.has("--id") || args.positionals.len() == 1;
            let positional = args.arity(if contextual { 1 } else { 2 }, 1001)?;
            let (session, tabs) = if contextual {
                (session_target(None, args.text("--id"))?, positional)
            } else {
                (
                    session_target(Some(&positional[0]), None)?,
                    &positional[1..],
                )
            };
            TerminalOp::Reorder {
                session,
                tabs: tabs.iter().map(|id| tab(id)).collect::<Result<_>>()?,
            }
        }
        "close" => TerminalOp::Close(tab(&args.arity(1, 1)?[0])?),
        "write" => {
            let stdin = args.has("--stdin");
            let positional = args.arity(if stdin { 1 } else { 2 }, if stdin { 1 } else { 2 })?;
            TerminalOp::Write {
                tab: tab(&positional[0])?,
                data: positional.get(1).cloned(),
                take: args.has("--take"),
            }
        }
        "read" => {
            let after = args
                .text("--after")
                .map(|value| {
                    value
                        .parse::<u64>()
                        .ok()
                        .filter(|n| *n <= shellfox_protocol::MAX_SAFE_INTEGER)
                        .ok_or_else(|| CliError::usage("--after requires an output sequence."))
                })
                .transpose()?;
            let generation = args
                .text("--generation")
                .map(|value| uuid(&value, "--generation requires a full UUID."))
                .transpose()?;
            TerminalOp::Read(shellfox_protocol::ReadTerminal {
                tab_id: tab(&args.arity(1, 1)?[0])?,
                after_sequence: after,
                generation,
            })
        }
        _ => return Err(CliError::usage("Unknown terminals operation.")),
    })
}

pub(super) fn path(operation: &str, rest: &[&str]) -> Result<PathOp> {
    let args = Arguments::parse(rest, &[], &[])?;
    if operation == "home" {
        args.arity(0, 0)?;
        return Ok(PathOp::Home);
    }
    let usage = || CliError::usage("Usage: shellfox path home|resolve|complete [path]");
    if !matches!(operation, "resolve" | "complete" | "create") {
        return Err(usage());
    }
    let value = args.arity(1, 1)?[0].clone();
    Ok(match operation {
        "resolve" => PathOp::Resolve(value),
        "complete" => PathOp::Complete(value),
        _ => PathOp::Create(value),
    })
}

/// `remote api request` forwards one v1 manager method; check it before pairing.
pub(super) fn remote_request(payload: &Value) -> Result<()> {
    let Some(method) = payload.get("method").and_then(Value::as_str) else {
        return Ok(());
    };
    let inner = payload
        .get("payload")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));
    match crate::manager::parse(method, inner)? {
        crate::manager::Call::Remote(_) => Ok(()),
        _ => Err(CliError::usage("Invalid remote input.")),
    }
}
