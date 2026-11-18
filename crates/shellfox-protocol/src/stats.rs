use crate::{methods::method, validation::*, *};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetSessionStats {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::ids::optional_wire_id"
    )]
    pub session_id: Option<SessionId>,
}
impl Validate for GetSessionStats {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
method!(GetSessionStats, SessionStats, "getSessionStats");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessStat {
    pub pid: u32,
    pub parent_pid: u32,
    pub name: String,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub memory_bytes: Option<u64>,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub elapsed_seconds: Option<f64>,
    pub domain: String,
}
fn measurement(memory: Option<u64>, elapsed: Option<f64>) -> Result<(), ValidationError> {
    if let Some(memory) = memory {
        sequence(memory)?;
    }
    if let Some(elapsed) = elapsed {
        ensure(elapsed.is_finite() && elapsed >= 0.0)?;
    }
    Ok(())
}
impl Validate for ProcessStat {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(
            self.pid > 0
                && self.name.encode_utf16().count() <= 4096
                && !self.domain.is_empty()
                && self.domain.encode_utf16().count() <= 4096,
        )?;
        measurement(self.memory_bytes, self.elapsed_seconds)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TabStat {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub tab_id: TabId,
    pub processes: Vec<ProcessStat>,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub memory_bytes: Option<u64>,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub elapsed_seconds: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::validation::optional"
    )]
    pub reason: Option<String>,
}
impl Validate for TabStat {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.processes.len() <= 100000)?;
        for process in &self.processes {
            process.validate()?;
        }
        if let Some(reason) = &self.reason {
            ensure(reason.encode_utf16().count() <= 65536)?;
        }
        measurement(self.memory_bytes, self.elapsed_seconds)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionStats {
    pub sessions: Vec<SessionDto>,
    pub tabs: Vec<TabStat>,
}
impl Validate for SessionStats {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.sessions.len() <= 10000 && self.tabs.len() <= 64)?;
        for session in &self.sessions {
            session.validate()?;
            ensure(session.env.is_empty())?;
        }
        for tab in &self.tabs {
            tab.validate()?;
        }
        Ok(())
    }
}
