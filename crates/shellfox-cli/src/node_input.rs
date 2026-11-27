//! Folder, stdin and `--input` reading, and the commands that never leave this machine.
use super::*;

/// 0.9 refused these with a remote `--host`; they never leave this machine.
pub(super) fn local_only(words: &[String]) -> Result<()> {
    let first = words.first().map_or("", String::as_str);
    let second = words.get(1).map_or("", String::as_str);
    let message = match (first, second) {
        ("host", "keep-awake") => "Keep awake only manages the local host.",
        ("host", "service") => "Service installation only manages the local host.",
        ("host", "run") => "host run only starts the local host.",
        ("host", _) => "Host lifecycle commands only operate on the local host.",
        ("network" | "machines" | "approvals", _) => "Network, machines, and approvals commands manage the local host. Use --host for session and terminal operations.",
        ("remote", "api") => "remote api manages the local host.",
        ("run" | "runs", _) => "Automation commands use the local host. Run the CLI on the target machine.",
        ("commands" | "cmd", _) => "Custom commands run locally, not through --host.",
        ("update", _) => "update is local-only; remote --host targets are rejected, not forwarded.",
        _ => return Ok(()),
    };
    Err(CliError::usage(message))
}

pub(super) fn create_session(rest: &[&str]) -> Result<Action> {
    let (positional, flags) = options(rest, &["--title", "--request-id"])?;
    let path = match positional.as_slice() {
        [] => None,
        [path] => Some((*path).to_owned()),
        _ => {
            return Err(CliError::usage(
                "Usage: shellfox session create [PATH] [--title TITLE] [--request-id UUID]",
            ))
        }
    };
    let title = flag(&flags, "--title");
    if title
        .as_deref()
        .is_some_and(|t| shellfox_core::validation::title(t).is_err())
    {
        return Err(CliError::usage("Invalid session title or request ID."));
    }
    let request_id = flag(&flags, "--request-id")
        .map(|id| {
            id.parse()
                .map_err(|_| CliError::usage("Invalid session title or request ID."))
        })
        .transpose()?;
    Ok(Action::CreateSession {
        path,
        title,
        request_id,
    })
}

/// A local folder, spelled as 0.9 realpath spelled it.
pub fn local_directory(path: Option<&str>) -> Result<String> {
    let cwd = std::path::absolute(path.unwrap_or("."))
        .ok()
        .filter(|cwd| cwd.is_dir())
        .ok_or_else(|| CliError::usage("Invalid directory."))?;
    crate::headless_layout::normalize(&cwd)
        .into_os_string()
        .into_string()
        .map_err(|_| CliError::usage("Invalid directory."))
}

/// Up to `limit` bytes of UTF-8 stdin, as 0.9 readStdin read it.
pub fn read_stdin(limit: usize) -> Result<String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(CliError::usage("Stdin exceeds the input limit."));
    }
    String::from_utf8(bytes).map_err(|_| CliError::usage("Stdin is not valid UTF-8."))
}

/// `--input` accepts inline JSON, `@file.json` or `-` for stdin, up to 1 MiB.
pub fn read_input(input: &str) -> Result<String> {
    use std::io::Read;
    const LIMIT: u64 = 1024 * 1024;
    // Unreadable or non-UTF-8 input is a usage error, like invalid JSON.
    let unreadable = |error: std::io::Error, source: &str| {
        CliError::usage(match error.kind() {
            std::io::ErrorKind::InvalidData => format!("{source} is not valid UTF-8."),
            _ => format!("Could not read {source}."),
        })
    };
    let mut text = String::new();
    if input == "-" {
        std::io::stdin()
            .take(LIMIT + 1)
            .read_to_string(&mut text)
            .map_err(|error| unreadable(error, "Stdin"))?;
    } else if let Some(file) = input.strip_prefix('@') {
        std::fs::File::open(file)
            .and_then(|opened| opened.take(LIMIT + 1).read_to_string(&mut text))
            .map_err(|error| unreadable(error, file))?;
    } else {
        text = input.to_owned();
    }
    if text.len() as u64 > LIMIT {
        return Err(CliError::usage("JSON input exceeds 1 MiB."));
    }
    Ok(text)
}
