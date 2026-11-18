use crate::{validation::*, *};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Waiting,
    Error,
    Running,
    Unknown,
    Settled,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TabStatus {
    Waiting,
    Error,
    Running,
    Unknown,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TabLifecycle {
    Launching,
    Open,
    Closed,
    LaunchUncertain,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalKind {
    Embedded,
    ExternalLegacy,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Counts {
    pub running: u64,
    pub waiting: u64,
    pub unknown: u64,
    pub error: u64,
    pub closed: u64,
    pub agents: u64,
}
impl Validate for Counts {
    fn validate(&self) -> Result<(), ValidationError> {
        for value in [
            self.running,
            self.waiting,
            self.unknown,
            self.error,
            self.closed,
            self.agents,
        ] {
            count(value)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TabDto {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub id: TabId,
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub session_id: SessionId,
    pub title: String,
    pub cwd: String,
    pub ordinal: u64,
    pub created_at: String,
    pub lifecycle: TabLifecycle,
    pub status: TabStatus,
    pub agents: u64,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub monitoring_reason: Option<String>,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub error: Option<AppError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::ids::optional_wire_id")]
    pub generation: Option<Generation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::validation::optional")]
    pub terminal_kind: Option<TerminalKind>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::validation::present_nullable"
    )]
    pub profile_id: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::validation::present_nullable"
    )]
    pub exit_code: Option<Option<i64>>,
}
impl Validate for TabDto {
    fn validate(&self) -> Result<(), ValidationError> {
        title(&self.title)?;
        cwd(&self.cwd)?;
        timestamp(&self.created_at)?;
        count(self.ordinal)?;
        count(self.agents)?;
        if let Some(reason) = &self.monitoring_reason {
            text(reason, 1000)?;
        }
        if let Some(error) = &self.error {
            error.validate()?;
        }
        if let Some(Some(code)) = self.exit_code {
            integer(code)?;
        }
        if let Some(Some(id)) = &self.profile_id {
            profile(id)?;
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WindowState {
    Alive,
    Closed,
    Unknown,
    Opening,
    LaunchUncertain,
    Unsupported,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionWindow {
    pub state: WindowState,
    pub can_reopen: bool,
    pub can_register: bool,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionDto {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub id: SessionId,
    pub title: String,
    pub cwd: String,
    pub adapter_id: AdapterId,
    pub shell_id: ShellId,
    pub created_at: String,
    pub updated_at: String,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub settled_at: Option<String>,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub pinned_at: Option<String>,
    pub status: Status,
    pub activity_status: TabStatus,
    pub counts: Counts,
    pub tabs: Vec<TabDto>,
    #[serde(default)]
    pub env: Vec<EnvVar>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::validation::optional")]
    pub window: Option<SessionWindow>,
    pub can_focus: bool,
    pub can_add_tab: bool,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub control_reason: Option<String>,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub error: Option<AppError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::validation::optional")]
    pub terminal_lifetime: Option<TerminalLifetime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::validation::optional")]
    pub shell_survival: Option<bool>,
}
impl Validate for SessionDto {
    fn validate(&self) -> Result<(), ValidationError> {
        title(&self.title)?;
        cwd(&self.cwd)?;
        timestamp(&self.created_at)?;
        timestamp(&self.updated_at)?;
        optional_time(&self.settled_at)?;
        optional_time(&self.pinned_at)?;
        self.counts.validate()?;
        ensure(self.tabs.len() <= 1000)?;
        for tab in &self.tabs {
            tab.validate()?;
        }
        env(&self.env, true)?;
        if let Some(window) = &self.window {
            text(&window.reason, 1000)?;
        }
        if let Some(reason) = &self.control_reason {
            text(reason, 1000)?;
        }
        if let Some(error) = &self.error {
            error.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagerSnapshot {
    pub revision: u64,
    pub sessions: Vec<SessionDto>,
    pub settings: SettingsDto,
    pub probe: NativeProbe,
    pub explorer: ExplorerIntegration,
    pub cli: CliIntegration,
}
impl Validate for ManagerSnapshot {
    fn validate(&self) -> Result<(), ValidationError> {
        sequence(self.revision)?;
        ensure(self.sessions.len() <= 10000)?;
        for session in &self.sessions {
            session.validate()?;
        }
        self.settings.validate()?;
        self.probe.validate()?;
        ensure(matches!(
            self.cli.command.as_str(),
            "shellfox" | "shellfox start <path>"
        ))?;
        for reason in [&self.explorer.reason, &self.cli.reason]
            .into_iter()
            .flatten()
        {
            text(reason, 1000)?;
        }
        Ok(())
    }
}
// Deliberately no From<core::Snapshot>: it lacks runtime/status/settings/probe evidence.
// A host adapter must supply those facts or return UNSUPPORTED, never invent them.
