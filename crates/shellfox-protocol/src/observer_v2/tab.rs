use super::*;
use crate::{validation::*, Validate, ValidationError};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tab {
    pub id: Id,
    pub session_id: Id,
    pub title: String,
    pub source_kind: Fact<SourceKind>,
    pub facts: TabFacts,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TabFacts {
    pub cwd: Fact<String>,
    pub ordinal: Fact<u64>,
    pub created_at: Fact<String>,
    pub lifecycle: Fact<crate::TabLifecycle>,
    pub status: Fact<crate::TabStatus>,
    pub agents: Fact<u64>,
    pub monitoring_reason: Fact<Option<String>>,
    pub error: Fact<Option<crate::AppError>>,
    pub generation: Fact<Option<Id>>,
    pub terminal_kind: Fact<Option<crate::TerminalKind>>,
    pub profile_id: Fact<Option<String>>,
    pub exit_code: Fact<Option<i64>>,
}
impl Validate for Tab {
    fn validate(&self) -> Result<(), ValidationError> {
        title(&self.title)?;
        self.facts.validate()
    }
}
impl Validate for TabFacts {
    fn validate(&self) -> Result<(), ValidationError> {
        self.cwd.check(|v| cwd(v))?;
        self.ordinal.check(|v| count(*v))?;
        self.created_at.check(|v| timestamp(v))?;
        self.agents.check(|v| count(*v))?;
        self.monitoring_reason.check(|v| {
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
        })?;
        self.profile_id.check(|v| {
            if let Some(v) = v {
                profile(v)?;
            }
            Ok(())
        })?;
        self.exit_code.check(|v| {
            if let Some(v) = v {
                integer(*v)?;
            }
            Ok(())
        })
    }
}
