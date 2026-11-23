//! The environment of a user's own terminal, layered the way 0.9 did it
//! (src/main/terminal/environment.ts): the user's environment, then the
//! terminal defaults, then Settings and session variables, then the tab marker.
use anyhow::Result;
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
};

pub(crate) const MARKER: &str = "SHELLFOX_TERMINAL_MARKER";
/// Shellfox opens OSC 8 links. Without these, pi and supports-hyperlinks
/// users print "label (url)" instead of a link. Settings can override them.
const HYPERLINK_VARIABLES: [(&str, &str); 2] = [("FORCE_HYPERLINK", "1"), ("PI_HYPERLINKS", "1")];
const MANAGED: [&str; 4] = ["TERM", "COLORTERM", "FORCE_HYPERLINK", "PI_HYPERLINKS"];

/// The environment a fresh login of this user gets.
///
/// Windows builds it from the registry with the user's token, like Windows
/// Terminal, so tabs see the current PATH and variables whichever shell
/// started Shellfox, and no PowerShell edition's PSModulePath leaks into
/// another. Unix inherits the host's environment, which carries the desktop
/// session (DISPLAY, WAYLAND_DISPLAY, SSH_AUTH_SOCK, DBUS_SESSION_BUS_ADDRESS).
pub(crate) fn user_base() -> Result<Vec<(OsString, OsString)>> {
    #[cfg(windows)]
    {
        windows_user_block()
    }
    #[cfg(not(windows))]
    {
        Ok(std::env::vars_os().collect())
    }
}

#[cfg(windows)]
fn windows_user_block() -> Result<Vec<(OsString, OsString)>> {
    use anyhow::ensure;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::{TOKEN_DUPLICATE, TOKEN_IMPERSONATE, TOKEN_QUERY},
        System::{
            Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock},
            Threading::{GetCurrentProcess, OpenProcessToken},
        },
    };
    let mut token = std::ptr::null_mut();
    // SAFETY: the pseudo-handle needs no close; `token` is a valid out pointer.
    let opened = unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_IMPERSONATE,
            &mut token,
        )
    };
    ensure!(opened != 0, "open the user token: {}", std::io::Error::last_os_error());
    let mut block = std::ptr::null_mut();
    // SAFETY: `token` is open; bInherit = FALSE ignores this process's variables.
    let created = unsafe { CreateEnvironmentBlock(&mut block, token, 0) };
    let error = std::io::Error::last_os_error();
    // SAFETY: `token` was opened above and is closed once.
    unsafe { CloseHandle(token) };
    ensure!(created != 0, "build the user environment: {error}");
    let mut entries = Vec::new();
    let mut cursor = block.cast::<u16>().cast_const();
    // SAFETY: the block is a sequence of NUL-terminated UTF-16 strings ending
    // with an empty one. Every read stays before that terminator.
    unsafe {
        loop {
            let mut len = 0;
            while *cursor.add(len) != 0 {
                len += 1;
            }
            if len == 0 {
                break;
            }
            let entry = std::slice::from_raw_parts(cursor, len);
            // Skip per-drive cwd entries such as "=C:=C:\".
            if let Some(split) = entry.iter().skip(1).position(|&c| c == u16::from(b'=')) {
                let split = split + 1;
                entries.push((
                    OsString::from_wide(&entry[..split]),
                    OsString::from_wide(&entry[split + 1..]),
                ));
            }
            cursor = cursor.add(len + 1);
        }
        DestroyEnvironmentBlock(block);
    }
    Ok(entries)
}

/// `base` is the user's environment, `configured` the merged Settings and
/// session variables. Names compare case-insensitively on Windows.
pub(crate) fn compose(
    base: impl IntoIterator<Item = (OsString, OsString)>,
    configured: &BTreeMap<OsString, OsString>,
    marker: Option<&str>,
) -> Vec<(OsString, OsString)> {
    compose_for(cfg!(windows), base, configured, marker)
}

fn compose_for(
    windows: bool,
    base: impl IntoIterator<Item = (OsString, OsString)>,
    configured: &BTreeMap<OsString, OsString>,
    marker: Option<&str>,
) -> Vec<(OsString, OsString)> {
    let mut env = Env { windows, entries: Vec::new() };
    for (key, value) in base {
        if !internal(&key) {
            env.set(key, value);
        }
    }
    // Terminal capabilities are defaults, not the host console's limits.
    env.set("TERM".into(), "xterm-256color".into());
    env.set("COLORTERM".into(), "truecolor".into());
    for (key, value) in HYPERLINK_VARIABLES {
        env.set(key.into(), value.into());
    }
    for (key, value) in configured {
        let key = match MANAGED.iter().find(|name| windows && eq(true, key, name)) {
            Some(name) => OsString::from(*name),
            None => key.clone(),
        };
        env.set(key, value.clone());
    }
    if let Some(marker) = marker {
        env.set(MARKER.into(), marker.into());
    }
    if windows {
        // Any Windows shell can start wsl.exe. List the variables Shellfox set
        // so they cross into the guest, keeping the user's other entries.
        let mut names: Vec<OsString> = MANAGED.iter().map(OsString::from).collect();
        names.extend(configured.keys().cloned());
        names.extend(marker.map(|_| OsString::from(MARKER)));
        names.retain(|name| !eq(true, name, "WSLENV"));
        let mut transfer: Vec<String> = Vec::new();
        for name in &names {
            let name = env.key(name).unwrap_or(name).to_string_lossy().into_owned();
            if !transfer.iter().any(|old| old.eq_ignore_ascii_case(&name)) {
                transfer.push(name);
            }
        }
        let previous = env.get("WSLENV").map(|v| v.to_string_lossy().into_owned());
        let mut entries: Vec<String> = previous
            .as_deref()
            .unwrap_or_default()
            .split(':')
            .filter(|entry| {
                let name = entry.split('/').next().unwrap_or_default();
                !entry.is_empty() && !transfer.iter().any(|t| t.eq_ignore_ascii_case(name))
            })
            .map(str::to_owned)
            .collect();
        entries.extend(transfer);
        let key = env.key(OsStr::new("WSLENV")).cloned().unwrap_or_else(|| "WSLENV".into());
        env.set(key, entries.join(":").into());
    }
    env.entries
}

/// Shellfox-internal values from the host never reach user programs.
fn internal(key: &OsStr) -> bool {
    let name = key.to_string_lossy().to_ascii_uppercase();
    name.starts_with("SHELLFOX_") || name == "ELECTRON_RUN_AS_NODE"
}

fn eq(windows: bool, a: &OsStr, b: &str) -> bool {
    if windows {
        a.to_string_lossy().eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

struct Env {
    windows: bool,
    entries: Vec<(OsString, OsString)>,
}
impl Env {
    fn set(&mut self, key: OsString, value: OsString) {
        let windows = self.windows;
        self.entries
            .retain(|(old, _)| !eq(windows, old, &key.to_string_lossy()));
        self.entries.push((key, value));
    }
    fn key(&self, name: &OsStr) -> Option<&OsString> {
        let name = name.to_string_lossy();
        self.entries
            .iter()
            .find(|(key, _)| eq(self.windows, key, &name))
            .map(|(key, _)| key)
    }
    fn get(&self, name: &str) -> Option<&OsString> {
        self.entries
            .iter()
            .find(|(key, _)| eq(self.windows, key, name))
            .map(|(_, value)| value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(entries: &[(&str, &str)]) -> BTreeMap<OsString, OsString> {
        entries.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect()
    }
    fn get<'a>(env: &'a [(OsString, OsString)], name: &str) -> Option<&'a str> {
        env.iter().find(|(k, _)| k == name).map(|(_, v)| v.to_str().unwrap())
    }
    fn base(entries: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        entries.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect()
    }

    #[test]
    fn user_values_stay_and_configured_values_win() {
        let env = compose_for(
            true,
            base(&[
                ("Path", r"C:\Windows"),
                ("USERPROFILE", r"C:\Users\me"),
                ("PSModulePath", r"C:\Program Files\WindowsPowerShell\Modules"),
                ("TERM", "dumb"),
            ]),
            &map(&[("PATH", r"C:\tools;C:\Windows"), ("NODE_OPTIONS", "--x"), ("term", "vt100")]),
            None,
        );
        assert_eq!(get(&env, "USERPROFILE"), Some(r"C:\Users\me"));
        assert_eq!(get(&env, "PSModulePath"), Some(r"C:\Program Files\WindowsPowerShell\Modules"));
        // Case variants are one Windows variable; the configured spelling wins.
        assert_eq!(get(&env, "PATH"), Some(r"C:\tools;C:\Windows"));
        assert_eq!(get(&env, "Path"), None);
        assert_eq!(get(&env, "NODE_OPTIONS"), Some("--x"));
        assert_eq!(get(&env, "TERM"), Some("vt100"));
        assert_eq!(get(&env, "term"), None);
        assert_eq!(get(&env, "COLORTERM"), Some("truecolor"));
        assert_eq!(get(&env, "FORCE_HYPERLINK"), Some("1"));
    }

    #[test]
    fn a_configured_psmodulepath_is_kept() {
        let env = compose_for(
            true,
            base(&[("PSModulePath", "default")]),
            &map(&[("PSModulePath", r"D:\modules")]),
            None,
        );
        assert_eq!(get(&env, "PSModulePath"), Some(r"D:\modules"));
    }

    #[test]
    fn host_internals_are_dropped_and_the_marker_is_ours() {
        let env = compose_for(
            false,
            base(&[
                ("HOME", "/home/me"),
                ("SHELLFOX_TERMINAL_MARKER", "stale"),
                ("SHELLFOX_SUPERVISOR_TOKEN", "secret"),
                ("ELECTRON_RUN_AS_NODE", "1"),
                ("DISPLAY", ":0"),
            ]),
            &BTreeMap::new(),
            Some("12345678-1234-4234-8234-123456789abc"),
        );
        assert_eq!(get(&env, "HOME"), Some("/home/me"));
        assert_eq!(get(&env, "DISPLAY"), Some(":0"));
        assert_eq!(get(&env, MARKER), Some("12345678-1234-4234-8234-123456789abc"));
        assert_eq!(get(&env, "SHELLFOX_SUPERVISOR_TOKEN"), None);
        assert_eq!(get(&env, "ELECTRON_RUN_AS_NODE"), None);
        // Unix names are case-sensitive and there is no WSLENV.
        assert_eq!(get(&env, "WSLENV"), None);
    }

    #[test]
    fn wslenv_lists_shellfox_values_once_and_keeps_user_entries() {
        let env = compose_for(
            true,
            base(&[("WSLENV", "KEEP/p:shellfox_terminal_marker/w:TERM/l::USERPROFILE/pu")]),
            &map(&[("Custom", "1")]),
            Some("m"),
        );
        assert_eq!(
            get(&env, "WSLENV"),
            Some("KEEP/p:USERPROFILE/pu:TERM:COLORTERM:FORCE_HYPERLINK:PI_HYPERLINKS:Custom:SHELLFOX_TERMINAL_MARKER")
        );
        let configured = compose_for(true, base(&[]), &map(&[("wslenv", "A/p")]), None);
        assert_eq!(
            get(&configured, "wslenv"),
            Some("A/p:TERM:COLORTERM:FORCE_HYPERLINK:PI_HYPERLINKS")
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_windows_user_block_is_the_registry_environment() {
        let block = user_base().unwrap();
        let value = |name: &str| {
            block
                .iter()
                .find(|(k, _)| k.to_string_lossy().eq_ignore_ascii_case(name))
                .map(|(_, v)| v.clone())
        };
        let profile = value("USERPROFILE").expect("USERPROFILE");
        assert!(std::path::Path::new(&profile).is_dir());
        assert!(value("PATH").is_some() && value("SystemRoot").is_some());
        assert!(value("APPDATA").is_some() && value("LOCALAPPDATA").is_some());
        assert!(block.iter().all(|(k, _)| !k.is_empty() && !k.to_string_lossy().starts_with('=')));
    }
}
