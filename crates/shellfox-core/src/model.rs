use crate::{
    validate_env, validation, CleanupToken, Generation, Preferences, ProfileId, Result, SessionId,
    TabId,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Session {
    pub id: SessionId,
    pub title: String,
    pub cwd: String,
    pub created_at: String,
    pub updated_at: String,
    pub settled_at: Option<String>,
    pub pinned_at: Option<String>,
    pub env: Vec<EnvVar>,
    pub tabs: Vec<Tab>,
}
impl Session {
    pub fn validate(&self) -> Result<()> {
        validation::title(&self.title)?;
        validation::cwd(&self.cwd)?;
        validation::timestamp(&self.created_at)?;
        validation::timestamp(&self.updated_at)?;
        for value in [&self.settled_at, &self.pinned_at].into_iter().flatten() {
            validation::timestamp(value)?;
        }
        validate_env(&self.env, false)?;
        let mut ids = HashSet::new();
        for (ordinal, tab) in self.tabs.iter().enumerate() {
            tab.validate()?;
            validation::ensure(
                tab.session_id == self.id && tab.ordinal as usize == ordinal && ids.insert(tab.id),
                "Invalid tab membership or order.",
            )?;
        }
        Ok(())
    }
}
/// Metadata intent only. Neither state asserts a live local process.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TabState {
    Pending,
    Archived,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tab {
    pub id: TabId,
    pub session_id: SessionId,
    pub title: String,
    pub cwd: String,
    pub ordinal: u32,
    /// Tab intent reservation time, not session birth or runtime-generation birth.
    /// Older prototype rows have no verified value and remain unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    pub profile_id: ProfileId,
    pub state: TabState,
}
impl Tab {
    pub fn validate(&self) -> Result<()> {
        validation::title(&self.title)?;
        validation::cwd(&self.cwd)?;
        if let Some(created_at) = &self.created_at {
            validation::timestamp(created_at)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Selection {
    pub session_id: Option<SessionId>,
    pub tab_id: Option<TabId>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub revision: u64,
    pub sessions: Vec<Session>,
    pub preferences: Preferences,
    pub selection: Selection,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateSession {
    pub request_id: SessionId,
    pub title: Option<String>,
    pub cwd: String,
    pub profile_id: ProfileId,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddTab {
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub profile_id: ProfileId,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvVar {
    #[serde(deserialize_with = "crate::settings_env::deserialize_name")]
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalLaunch {
    pub session_id: SessionId,
    pub tab_id: TabId,
    pub profile_id: ProfileId,
    pub cwd: String,
    /// Explicit user-saved variables only, never inherited process environment.
    pub env: Vec<EnvVar>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalIdentity {
    pub tab_id: TabId,
    pub generation: Generation,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreparedClose {
    pub token: CleanupToken,
    pub terminals: Vec<TerminalIdentity>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeReason {
    Sessions,
    Settings,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangedEvent {
    pub revision: u64,
    pub reason: ChangeReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub select_session_id: Option<SessionId>,
}
