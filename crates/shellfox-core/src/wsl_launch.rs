use crate::{validation::ensure, ProfileId, Result};
use serde::{Deserialize, Deserializer, Serialize};
use std::num::NonZeroU32;

/// Persisted only by the local launch owner. Missing means a pre-binding tab.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum WslLaunchBinding {
    Pending,
    Bound { account: WslLaunchAccount },
}

impl WslLaunchBinding {
    pub fn for_new_tab(profile: &ProfileId) -> Option<Self> {
        profile
            .as_str()
            .starts_with("wsl:")
            .then_some(Self::Pending)
    }
}

/// WSL selects by account name; the actual effective UID must also match.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WslLaunchAccount {
    uid: NonZeroU32,
    #[serde(deserialize_with = "deserialize_name")]
    name: String,
}

impl WslLaunchAccount {
    pub fn uid(&self) -> u32 {
        self.uid.get()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn verify_uid(&self, effective_uid: u32) -> Result<()> {
        ensure(
            self.uid() == effective_uid,
            "The WSL guest user does not match the tab's launch account.",
        )
    }
}

fn deserialize_name<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<String, D::Error> {
    let name = String::deserialize(deserializer)?;
    if name.is_empty()
        || name.len() > 128
        || name.starts_with('-')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
        || matches!(name.as_str(), "." | "..")
    {
        return Err(serde::de::Error::custom("Invalid WSL account name"));
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_account_rejects_root_missing_uid_and_flag_like_names() {
        for bad in [
            r#"{"uid":0,"name":"root"}"#,
            r#"{"name":"alpha"}"#,
            r#"{"uid":1101,"name":"--exec"}"#,
        ] {
            assert!(serde_json::from_str::<WslLaunchAccount>(bad).is_err());
        }
        let account: WslLaunchAccount =
            serde_json::from_str(r#"{"uid":1101,"name":"alpha"}"#).unwrap();
        assert!(account.verify_uid(1101).is_ok());
        assert!(account.verify_uid(1102).is_err());
        assert!(account.verify_uid(0).is_err());
    }
}
