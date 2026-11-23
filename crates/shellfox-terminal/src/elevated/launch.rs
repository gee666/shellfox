use super::wire::{encode, AdapterError};
#[cfg(windows)]
use crate::EnvironmentPolicy;
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use std::path::Path;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    RetainedV1,
    IsolatedV1,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IsolatedProbe {
    protocol: String,
    version: u8,
    shell: String,
    no_profile: bool,
}
impl Capability {
    /// Parsing a probe is protocol validation, not a loader-policy grant or UAC
    /// approval. The real factory additionally binds it to the verified bundle.
    pub fn parse_probe(output: &[u8]) -> Result<Self, AdapterError> {
        if std::str::from_utf8(output).map(str::trim) == Ok("shellfox-elevated-conpty-v1") {
            return Ok(Self::RetainedV1);
        }
        if output.len() > 2048 {
            return Err(AdapterError::InvalidProtocol);
        }
        let probe: IsolatedProbe =
            serde_json::from_slice(output).map_err(|_| AdapterError::InvalidProtocol)?;
        if probe.protocol != "shellfox-elevated-isolated"
            || probe.version != 1
            || probe.shell != "windows-powershell"
            || !probe.no_profile
        {
            return Err(AdapterError::InvalidProtocol);
        }
        Ok(Self::IsolatedV1)
    }
    pub fn require_isolated(self) -> Result<(), AdapterError> {
        match self {
            Self::IsolatedV1 => Ok(()),
            Self::RetainedV1 => Err(AdapterError::IsolatedModeUnavailable),
        }
    }
}

#[derive(Serialize)]
pub struct Launch {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(rename = "isolationRoot", skip_serializing_if = "Option::is_none")]
    isolation_root: Option<PathBuf>,
    cwd: PathBuf,
    env: BTreeMap<String, String>,
    cols: u16,
    rows: u16,
}
impl Launch {
    #[cfg(windows)]
    pub fn isolated(
        cwd: &Path,
        root: &Path,
        cols: u16,
        rows: u16,
        policy: EnvironmentPolicy,
    ) -> Result<Self, AdapterError> {
        let root = super::isolated_env::local_directory(root)?;
        let cwd = super::isolated_env::local_directory(cwd)?;
        if root.parent().is_none() {
            return Err(AdapterError::InvalidLaunch);
        }
        let env = super::isolated_env::environment(&root, policy)?;
        Self::validated("launch-isolated", cwd, Some(root), env, cols, rows)
    }

    #[cfg(windows)]
    pub(super) fn interactive(config: &crate::PtyConfig) -> Result<Self, AdapterError> {
        let cwd = crate::environment::lookup_path(config.cwd.clone());
        if !cwd.is_dir() {
            return Err(AdapterError::InvalidLaunch);
        }
        // The user's own environment and profile, like 0.9. Settings and session
        // variables can come from a remote device, so they never reach an
        // elevated shell.
        let mut env = BTreeMap::new();
        let user = crate::environment_user::compose(
            crate::environment_user::user_base().map_err(|_| AdapterError::InvalidLaunch)?,
            &BTreeMap::new(),
            config.terminal_marker.as_deref(),
        );
        for (key, value) in user {
            let (Some(key), Some(value)) = (key.to_str(), value.to_str()) else {
                continue;
            };
            if !key.starts_with('=') && !runtime_hook(key) {
                env.insert(key.to_uppercase(), value.into());
            }
        }
        Self::validated("launch", cwd, None, env, config.cols, config.rows)
    }

    #[cfg(any(windows, test, feature = "test-support"))]
    fn validated(
        kind: &'static str,
        cwd: PathBuf,
        isolation_root: Option<PathBuf>,
        env: BTreeMap<String, String>,
        cols: u16,
        rows: u16,
    ) -> Result<Self, AdapterError> {
        for path in std::iter::once(&cwd).chain(isolation_root.as_ref()) {
            if !path.is_absolute()
                || path.to_str().is_none_or(|s| {
                    s.is_empty()
                        || s.encode_utf16().count() > 32767
                        || s.chars().any(char::is_control)
                })
            {
                return Err(AdapterError::InvalidLaunch);
            }
        }
        dimensions(cols, rows)?;
        if env.len() > 1024 {
            return Err(AdapterError::InvalidLaunch);
        }
        let mut names = std::collections::BTreeSet::new();
        let mut units = 1usize;
        for (key, value) in &env {
            let key_units = key.encode_utf16().count();
            if key_units == 0
                || key_units > 32767
                || key.chars().any(|c| c.is_control() || c == '=')
                || value.contains('\0')
                || !names.insert(key.to_uppercase())
                || runtime_hook(key)
            {
                return Err(AdapterError::InvalidLaunch);
            }
            units = units
                .checked_add(key_units + value.encode_utf16().count() + 2)
                .ok_or(AdapterError::InvalidLaunch)?;
            if units > 32767 {
                return Err(AdapterError::InvalidLaunch);
            }
        }
        Ok(Self {
            kind,
            isolation_root,
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
        Self::validated("launch-isolated", cwd.clone(), Some(cwd), env, cols, rows)
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
