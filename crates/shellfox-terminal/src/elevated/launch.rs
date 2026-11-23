use super::wire::{encode, AdapterError};
use serde::Serialize;
use std::{collections::BTreeMap, path::PathBuf};

/// The helper's `probe` line. Parsing it is protocol validation, not a
/// loader-policy grant or UAC approval.
#[cfg(any(windows, test))]
pub(super) fn parse_probe(output: &[u8]) -> Result<(), AdapterError> {
    if std::str::from_utf8(output).map(str::trim) == Ok("shellfox-elevated-conpty-v1") {
        Ok(())
    } else {
        Err(AdapterError::InvalidProtocol)
    }
}

#[derive(Serialize)]
pub struct Launch {
    #[serde(rename = "type")]
    kind: &'static str,
    cwd: PathBuf,
    env: BTreeMap<String, String>,
    cols: u16,
    rows: u16,
}
impl Launch {
    #[cfg(windows)]
    pub(super) fn interactive(config: &crate::PtyConfig) -> Result<Self, AdapterError> {
        let cwd = crate::environment::lookup_path(config.cwd.clone());
        if !cwd.is_dir() {
            return Err(AdapterError::InvalidLaunch);
        }
        // The user's own environment and profile, as a UAC-started PowerShell
        // and 0.9 have them. Settings and session variables can come from a
        // remote device, so they never reach an elevated shell.
        let mut env = BTreeMap::new();
        let user = crate::environment_user::compose(
            crate::environment_user::user_base(),
            crate::environment_user::Layers {
                configured: None,
                cli_bin: config.cli_bin.as_deref(),
                marker: config.terminal_marker.as_deref(),
                host_paths: Some(&config.host_paths),
            },
        );
        for (key, value) in user {
            let (Some(key), Some(value)) = (key.to_str(), value.to_str()) else {
                continue;
            };
            if !key.starts_with('=') && !runtime_hook(key) {
                env.insert(key.to_uppercase(), value.into());
            }
        }
        Self::validated(cwd, env, config.cols, config.rows)
    }

    #[cfg(any(windows, test, feature = "test-support"))]
    fn validated(
        cwd: PathBuf,
        env: BTreeMap<String, String>,
        cols: u16,
        rows: u16,
    ) -> Result<Self, AdapterError> {
        if !cwd.is_absolute()
            || cwd.to_str().is_none_or(|s| {
                s.is_empty() || s.encode_utf16().count() > 32767 || s.chars().any(char::is_control)
            })
        {
            return Err(AdapterError::InvalidLaunch);
        }
        dimensions(cols, rows)?;
        if env.len() > 1024 {
            return Err(AdapterError::InvalidLaunch);
        }
        // Windows limits each name and value to 32767 UTF-16 units, not the
        // block; the frame bound limits the total.
        let mut names = std::collections::BTreeSet::new();
        for (key, value) in &env {
            let key_units = key.encode_utf16().count();
            if key_units == 0
                || key_units > 32767
                || value.encode_utf16().count() > 32767
                || key.chars().any(|c| c.is_control() || c == '=')
                || value.contains('\0')
                || !names.insert(key.to_uppercase())
                || runtime_hook(key)
            {
                return Err(AdapterError::InvalidLaunch);
            }
        }
        Ok(Self {
            kind: "launch",
            cwd,
            env,
            cols,
            rows,
        })
    }
    pub fn prepare(self) -> Result<PreparedLaunch, AdapterError> {
        Ok(PreparedLaunch {
            bytes: encode(&self).map_err(|_| AdapterError::InvalidLaunch)?,
            environment: self.env,
        })
    }
    /// Test-only constructor. It cannot grant loader review or UAC approval.
    #[cfg(any(test, feature = "test-support"))]
    pub fn fixture(
        cwd: PathBuf,
        env: BTreeMap<String, String>,
        cols: u16,
        rows: u16,
    ) -> Result<Self, AdapterError> {
        Self::validated(cwd, env, cols, rows)
    }
}
/// Serialized and bounded before acquiring a process. No environment in Debug.
pub struct PreparedLaunch {
    pub(crate) bytes: Vec<u8>,
    pub(super) environment: BTreeMap<String, String>,
}
impl PreparedLaunch {
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn launcher_environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }
}

pub(crate) fn dimensions(cols: u16, rows: u16) -> Result<(), AdapterError> {
    if !(2..=500).contains(&cols) || !(2..=500).contains(&rows) {
        return Err(AdapterError::InvalidInput);
    }
    Ok(())
}
#[cfg(any(windows, test, feature = "test-support"))]
pub(super) fn runtime_hook(key: &str) -> bool {
    let name = key.to_ascii_uppercase();
    name == "DEVPATH"
        || ["COR_", "CORECLR_", "COMPLUS_", "DOTNET_"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn interactive_launch_gets_the_user_environment_without_session_variables() {
        let cwd = std::env::current_dir().unwrap();
        let mut config = crate::PtyConfig::user(&cwd);
        config.terminal_marker = Some("12345678-1234-4234-8234-123456789abc".into());
        config
            .env
            .insert("SHELLFOX_TEST_UNTRUSTED_OVERRIDE".into(), "remote".into());
        let launch = Launch::interactive(&config).unwrap().prepare().unwrap();
        let frame: serde_json::Value = serde_json::from_slice(launch.as_bytes()).unwrap();
        assert_eq!(frame["type"], "launch");
        assert_eq!(frame["cols"], 80);
        assert_eq!(frame["rows"], 24);
        assert_eq!(frame.as_object().unwrap().len(), 5);
        let env = frame["env"].as_object().unwrap();
        assert!(!env.contains_key("SHELLFOX_TEST_UNTRUSTED_OVERRIDE"));
        assert!(env.keys().all(|key| !runtime_hook(key)));
        assert_eq!(env["TERM"], "xterm-256color");
        assert_eq!(env["SHELLFOX_TERMINAL_MARKER"], "12345678-1234-4234-8234-123456789abc");
        let profile = env["USERPROFILE"].as_str().unwrap();
        assert!(std::path::Path::new(profile).is_dir());
        // The registry value, not one inherited from whichever shell started the test.
        let modules = env.get("PSMODULEPATH").and_then(|v| v.as_str()).unwrap_or_default();
        assert!(!modules.to_ascii_lowercase().contains(r"\powershell\7"), "{modules}");
    }
}
