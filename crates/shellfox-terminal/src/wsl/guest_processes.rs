//! Status-only evidence about a WSL guest's processes that carry a Shellfox
//! tab marker, collected as 0.9 did. It never proves ownership and grants no
//! authority to signal a guest process.
use super::WslError;
use base64::{engine::general_purpose::STANDARD, Engine};

#[cfg(windows)]
const PYTHON: &str = include_str!("../../../../native/wsl/guest-processes.py");
#[cfg(windows)]
const SHELL: &str = include_str!("../../../../native/wsl/guest-processes.sh");
const HEADER: &str = "SHELLFOX_GUEST_PROCESSES_1";
const MARKER: &[u8] = b"SHELLFOX_TERMINAL_MARKER=";
const MAX_OUTPUT: usize = 8 * 1024 * 1024;
const MAX_ROWS: usize = 20_000;
const MAX_FIELD: usize = 64 * 1024;
const MAX_ARGS: usize = 4096;
/// The base64 length of a field one byte over the limit, as `head -c` reads
/// it, plus its two-byte read-status suffix.
const MAX_ENCODED: usize = (MAX_FIELD + 3).div_ceil(3) * 4;
const MAX_MARKER_ENCODED: usize = 8 * 1024;

/// One guest process that carries a tab marker. Sensitive and transient:
/// deliberately neither Debug nor Serialize.
pub struct GuestProcess {
    /// The SHELLFOX_TERMINAL_MARKER value, a tab UUID when Shellfox set it.
    pub marker: String,
    /// False when the process changed mid-read or a field isn't exact text.
    pub accessible: bool,
    pub executable: Option<String>,
    pub process_name: Option<String>,
    pub argv: Option<Vec<String>>,
}
pub struct GuestProcesses {
    pub processes: Vec<GuestProcess>,
    /// False when some process could not be read, so a marked one may be missing.
    pub complete: bool,
}

impl super::OsWsl {
    /// Lists the marked processes of `account`, or of the distribution's
    /// default user, in `distro`: guest Python first, then `/bin/sh` if
    /// Python fails. The fixed scripts get no arguments.
    #[cfg(windows)]
    pub fn guest_processes(
        &self,
        distro: &super::WslProfileId,
        account: Option<&str>,
    ) -> Result<GuestProcesses, WslError> {
        if account.is_some_and(|account| {
            account.is_empty()
                || account.len() > 128
                || account.starts_with('-')
                || !account
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
        }) {
            return Err(WslError::InvalidRequest);
        }
        let run = |program: &[&str]| {
            let mut args = vec!["--distribution", distro.distro()];
            if let Some(account) = account {
                args.extend(["--user", account]);
            }
            args.extend(["--cd", "/", "--exec"]);
            args.extend_from_slice(program);
            super::discovery_process::bounded_command(self.command(&args), MAX_OUTPUT)
                .and_then(|output| parse_guest_processes(&output))
        };
        match run(&["/usr/bin/python3", "-I", "-S", "-c", PYTHON]) {
            Err(WslError::Deadline) => Err(WslError::Deadline),
            Err(_) => run(&["/bin/sh", "-c", SHELL]),
            listed => listed,
        }
    }
}

/// Parses the scripts' output: a header, `R` rows, `E` for an unreadable
/// process and a final `END`. Anything else rejects the whole listing.
pub fn parse_guest_processes(output: &[u8]) -> Result<GuestProcesses, WslError> {
    if output.len() > MAX_OUTPUT {
        return Err(WslError::Protocol);
    }
    let text = std::str::from_utf8(output).map_err(|_| WslError::Protocol)?;
    let mut lines = text.strip_suffix('\n').unwrap_or(text).split('\n');
    if lines.next() != Some(HEADER) || lines.next_back() != Some("END") {
        return Err(WslError::Protocol);
    }
    let mut listing = GuestProcesses {
        processes: Vec::new(),
        complete: true,
    };
    for line in lines {
        if line == "E" {
            listing.complete = false;
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let ["R", ok @ ("0" | "1"), executable, argv, marker, name] = fields[..] else {
            return Err(WslError::Protocol);
        };
        if listing.processes.len() >= MAX_ROWS {
            return Err(WslError::Protocol);
        }
        let (executable, argv, name) = (decode(executable)?, decode(argv)?, decode(name)?);
        if marker.len() > MAX_MARKER_ENCODED {
            listing.complete = false;
            continue;
        }
        let Some(marker) = marker_value(&decode(marker)?.unwrap_or_default()) else {
            continue;
        };
        let mut exact = true;
        let mut text = |bytes: Vec<u8>| {
            String::from_utf8(bytes).unwrap_or_else(|error| {
                exact = false;
                String::from_utf8_lossy(error.as_bytes()).into_owned()
            })
        };
        let executable = executable.map(&mut text);
        let process_name = name.map(&mut text);
        let argv = argv
            .and_then(arguments)
            .map(|args| args.into_iter().map(&mut text).collect());
        listing.processes.push(GuestProcess {
            marker,
            accessible: ok == "1" && exact,
            executable,
            process_name,
            argv,
        });
    }
    Ok(listing)
}

fn decode(field: &str) -> Result<Option<Vec<u8>>, WslError> {
    if field == "-" {
        return Ok(None);
    }
    if field.len() > MAX_ENCODED {
        return Err(WslError::Protocol);
    }
    STANDARD
        .decode(field)
        .map(Some)
        .map_err(|_| WslError::Protocol)
}

/// Exactly one marker entry, as in 0.9; none or several carry no tab.
fn marker_value(entries: &[u8]) -> Option<String> {
    let mut entries = entries.split(|&b| b == 0).filter(|entry| !entry.is_empty());
    let entry = entries.next()?;
    let value = entry.strip_prefix(MARKER)?;
    (entries.next().is_none()
        && !value.is_empty()
        && value.len() <= 64
        && value.iter().all(|b| b.is_ascii_graphic()))
    .then(|| String::from_utf8_lossy(value).into_owned())
}

/// The raw command line ends with NUL and `1` when it was read whole.
fn arguments(field: Vec<u8>) -> Option<Vec<Vec<u8>>> {
    let raw = field.strip_suffix(b"\x001")?;
    if raw.len() > MAX_FIELD {
        return None;
    }
    let mut args: Vec<Vec<u8>> = raw.split(|&b| b == 0).map(<[u8]>::to_vec).collect();
    if args.last().is_some_and(Vec::is_empty) {
        args.pop();
    }
    (args.len() <= MAX_ARGS).then_some(args)
}

#[cfg(test)]
#[path = "guest_processes_tests.rs"]
mod tests;
