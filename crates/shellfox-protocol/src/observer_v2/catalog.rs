use super::*;
use crate::{validation::*, Validate, ValidationError};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CatalogEntry {
    Session {
        value: Box<Session>,
    },
    #[serde(rename_all = "camelCase")]
    Unavailable {
        session_id: Id,
        issues: Vec<ProjectionIssue>,
    },
}
impl CatalogEntry {
    pub fn id(&self) -> Id {
        match self {
            Self::Session { value } => value.id,
            Self::Unavailable { session_id, .. } => *session_id,
        }
    }
}
impl Validate for CatalogEntry {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Session { value } => value.validate(),
            Self::Unavailable { issues, .. } => {
                ensure(!issues.is_empty() && issues.len() <= 64)?;
                for issue in issues {
                    issue.validate()?;
                }
                Ok(())
            }
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Coverage {
    pub mode: CatalogMode,
    pub entry_count: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub revision: u64,
    pub coverage: Coverage,
    pub entries: Vec<CatalogEntry>,
    pub settings: Fact<Settings>,
    pub probe: Fact<crate::NativeProbe>,
    pub explorer: Fact<crate::ExplorerIntegration>,
    pub cli: Fact<crate::CliIntegration>,
}
impl Validate for Snapshot {
    fn validate(&self) -> Result<(), ValidationError> {
        sequence(self.revision)?;
        ensure(self.entries.len() <= 10000 && self.coverage.entry_count == self.entries.len())?;
        let mut ids = std::collections::HashSet::new();
        let mut tabs = std::collections::HashSet::new();
        for entry in &self.entries {
            entry.validate()?;
            ensure(ids.insert(entry.id()))?;
            if let CatalogEntry::Session { value } = entry {
                for tab in &value.tabs {
                    ensure(tabs.insert(tab.id))?;
                }
            }
        }
        self.settings.validate()?;
        self.probe.validate()?;
        self.explorer.check(|v| {
            if let Some(reason) = &v.reason {
                text(reason, 1000)?;
            }
            Ok(())
        })?;
        self.cli.check(|v| {
            ensure(matches!(
                v.command.as_str(),
                "shellfox" | "shellfox start <path>"
            ))?;
            if let Some(reason) = &v.reason {
                text(reason, 1000)?;
            }
            Ok(())
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionStats {
    pub sessions: Vec<Session>,
    pub tabs: Vec<crate::TabStat>,
}
impl Validate for SessionStats {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.sessions.len() <= 10000 && self.tabs.len() <= 64)?;
        let mut ids = std::collections::HashSet::new();
        for session in &self.sessions {
            session.validate()?;
            ensure(ids.insert(session.id))?;
            session.facts.env.check(|env| ensure(env.is_empty()))?;
        }
        for tab in &self.tabs {
            tab.validate()?;
        }
        Ok(())
    }
}
