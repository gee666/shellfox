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
    #[serde(rename = "isolationRoot")]
    isolation_root: PathBuf,
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
        Self::validated(cwd, root, env, cols, rows)
    }
    #[cfg(any(windows, test, feature = "test-support"))]
    fn validated(
        cwd: PathBuf,
        isolation_root: PathBuf,
        env: BTreeMap<String, String>,
        cols: u16,
        rows: u16,
    ) -> Result<Self, AdapterError> {
        for path in [&cwd, &isolation_root] {
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
            kind: "launch-isolated",
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
        Self::validated(cwd.clone(), cwd, env, cols, rows)
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
fn runtime_hook(key: &str) -> bool {
    let name = key.to_ascii_uppercase();
    name == "DEVPATH"
        || ["COR_", "CORECLR_", "COMPLUS_", "DOTNET_"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
}
