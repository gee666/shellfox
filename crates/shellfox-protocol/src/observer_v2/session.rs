use super::*;
use crate::{validation::*, Validate, ValidationError};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub id: Id,
    pub title: String,
    pub facts: SessionFacts,
    pub tabs: Vec<Tab>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionFacts {
    pub cwd: Fact<String>,
    pub adapter_id: Fact<crate::AdapterId>,
    pub shell_id: Fact<crate::ShellId>,
    pub created_at: Fact<String>,
    pub updated_at: Fact<String>,
    pub settled_at: Fact<Option<String>>,
    pub pinned_at: Fact<Option<String>>,
    pub status: Fact<crate::Status>,
    pub activity_status: Fact<crate::TabStatus>,
    pub counts: Fact<crate::Counts>,
    pub env: Fact<Vec<crate::EnvVar>>,
    pub window: Fact<Option<crate::SessionWindow>>,
    pub can_focus: Fact<bool>,
    pub can_add_tab: Fact<bool>,
    pub control_reason: Fact<Option<String>>,
    pub error: Fact<Option<crate::AppError>>,
    pub terminal_lifetime: Fact<Option<crate::TerminalLifetime>>,
    pub shell_survival: Fact<Option<bool>>,
}
impl Validate for SessionFacts {
    fn validate(&self) -> Result<(), ValidationError> {
        self.cwd.check(|v| cwd(v))?;
        self.created_at.check(|v| timestamp(v))?;
        self.updated_at.check(|v| timestamp(v))?;
        self.settled_at.check(optional_time)?;
        self.pinned_at.check(optional_time)?;
        self.counts.validate()?;
        self.env.check(|v| env(v, true))?;
        self.window.check(|v| {
            if let Some(v) = v {
                text(&v.reason, 1000)?;
            }
            Ok(())
        })?;
        self.control_reason.check(|v| {
            if let Some(v) = v {
                text(v, 1000)?;
            }
            Ok(())
        })?;
        self.error.check(|v| {
            if let Some(v) = v {
                v.validate()?;
            }
            Ok(())
        })
    }
}
impl Validate for Session {
    fn validate(&self) -> Result<(), ValidationError> {
        title(&self.title)?;
        self.facts.validate()?;
        ensure(self.tabs.len() <= 1000)?;
        let mut ids = std::collections::HashSet::new();
        for tab in &self.tabs {
            tab.validate()?;
            ensure(tab.session_id == self.id && ids.insert(tab.id))?;
        }
        Ok(())
    }
}
