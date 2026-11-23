use anyhow::{bail, Result};
use shellfox_ssh::{ConnectionOptions, ImportSession, UnsupportedProxy};
use std::collections::HashMap;
#[cfg(all(windows, debug_assertions, feature = "test-putty-source"))]
mod fixture;
#[cfg(windows)]
mod windows;

pub(super) fn import_sessions() -> Result<Vec<ImportSession>> {
    #[cfg(windows)]
    {
        let user = std::env::var("USERNAME").ok();
        #[cfg(all(debug_assertions, feature = "test-putty-source"))]
        if std::env::args_os().any(|arg| arg == "--ui-acceptance") {
            return import_from(fixture::read, user.as_deref());
        }
        import_from(windows::read, user.as_deref())
    }
    #[cfg(not(windows))]
    {
        let user = std::env::var("USER").ok();
        let home = std::env::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Home directory is unavailable."))?;
        import_from(
            || read_directory(&home.join(".putty/sessions")),
            user.as_deref(),
        )
    }
}

fn import_from(
    read: impl FnOnce() -> Result<Entries>,
    user: Option<&str>,
) -> Result<Vec<ImportSession>> {
    Ok(read()?
        .into_iter()
        .filter_map(|(name, values)| parse(&name, &values, user))
        .collect())
}

const MAX_SESSIONS: usize = 10_000;
const MAX_BYTES: usize = 8 * 1024 * 1024;
#[cfg(windows)]
const FIELDS: &[&str] = &[
    "HostName",
    "PortNumber",
    "UserName",
    "PublicKeyFile",
    "Protocol",
    "AddressFamily",
    "ProxyMethod",
    "ProxyType",
    "ProxySOCKSVersion",
    "ProxyHost",
    "ProxyPort",
    "AgentFwd",
    "TryAgent",
    "UserNameFromEnvironment",
];
type Entries = Vec<(String, HashMap<String, String>)>;

#[cfg(not(windows))]
fn read_directory(path: &std::path::Path) -> Result<Entries> {
    use std::io::Read;
    let files = match std::fs::read_dir(path) {
        Ok(files) => files,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };
    let mut result = Vec::new();
    let mut total = 0;
    for (index, entry) in files.enumerate() {
        if index >= MAX_SESSIONS {
            bail!("Too many PuTTY sessions.");
        }
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let mut bytes = Vec::new();
        let limit = MAX_BYTES - total;
        std::fs::File::open(entry.path())?
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            bail!("PuTTY sessions exceed the 8 MiB import limit.");
        }
        total += bytes.len();
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        result.push((
            name,
            text.lines()
                .filter_map(|line| line.split_once('='))
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
                .collect(),
        ));
    }
    Ok(result)
}

fn decode_name(name: &str) -> Option<String> {
    let mut decoded = Vec::with_capacity(name.len());
    let mut bytes = name.as_bytes().iter().copied();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let digit = |byte| char::from(byte).to_digit(16).map(|value| value as u8);
            decoded.push(digit(bytes.next()?)? * 16 + digit(bytes.next()?)?);
        } else {
            decoded.push(byte);
        }
    }
    String::from_utf8(decoded).ok()
}
fn parse(
    name: &str,
    values: &HashMap<String, String>,
    local_user: Option<&str>,
) -> Option<ImportSession> {
    let get = |key| values.get(key).map(String::as_str);
    let number = |key| get(key).and_then(|value| value.parse::<u16>().ok());
    if get("Protocol").is_some_and(|value| !value.trim().eq_ignore_ascii_case("ssh")) {
        return None;
    }
    let name = decode_name(name).unwrap_or_else(|| name.to_owned());
    let host = get("HostName")?.trim();
    let port = get("PortNumber").unwrap_or("22").parse().ok()?;
    let mut user = get("UserName").unwrap_or_default();
    if user.is_empty() && !host.contains('@') && number("UserNameFromEnvironment") == Some(1) {
        user = local_user.unwrap_or_default();
    }
    let proxy = number("ProxyMethod")
        .filter(|&n| n <= 8)
        .unwrap_or_else(|| match number("ProxyType") {
            Some(1) => 3,
            Some(2) if number("ProxySOCKSVersion") == Some(4) => 1,
            Some(2) => 2,
            Some(3) => 4,
            Some(4) => 5,
            _ => 0,
        });
    let options = ConnectionOptions {
        address_family: match number("AddressFamily") {
            Some(1) => Some(4),
            Some(2) => Some(6),
            _ => None,
        },
        agent_forward: (number("AgentFwd") == Some(1)).then_some(true),
        try_agent: (number("TryAgent") == Some(0)).then_some(false),
        unsupported_proxy: (proxy != 0).then(|| UnsupportedProxy {
            method: [
                "none",
                "SOCKS4",
                "SOCKS5",
                "HTTP",
                "Telnet",
                "local command",
                "SSH jump host",
                "SSH command",
                "SSH subsystem",
            ][usize::from(proxy)]
            .into(),
            host: get("ProxyHost").map(|host| {
                host.chars()
                    .take(1000)
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .collect()
            }),
            port: get("ProxyPort").map_or(Some(80), |_| number("ProxyPort")),
        }),
    };
    Some(ImportSession {
        name,
        host: host.into(),
        port,
        user: user.into(),
        key_file: get("PublicKeyFile")
            .filter(|v| !v.is_empty())
            .map(str::to_owned),
        connection_options: (options != ConnectionOptions::default()).then_some(options),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imports_escaped_names_and_preserves_nonexecuted_proxy_settings() {
        let values = [
            ("HostName", "deploy@example.net"),
            ("PortNumber", "2222"),
            ("ProxyMethod", "6"),
            ("AddressFamily", "2"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        let session = parse("%E6%B5%8B%E8%AF%95%20test", &values, None).unwrap();
        assert_eq!(session.name, "测试 test");
        assert_eq!(session.port, 2222);
        let options = session.connection_options.unwrap();
        assert_eq!(options.address_family, Some(6));
        assert_eq!(options.unsupported_proxy.unwrap().method, "SSH jump host");
    }
}
