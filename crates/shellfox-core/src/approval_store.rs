//! Host-local persisted approval data, not a remote request or a capability.
use crate::{validation::ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const APPROVAL_RECORD_LIMIT: usize = 1000;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ApprovalStorePolicy {
    #[serde(rename = "control-full-account-v1")]
    ControlFullAccountV1,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApprovalStoreRecord {
    pub peer_id: String,
    pub hostname: String,
    pub approved_at_millis: u64,
    pub policy: ApprovalStorePolicy,
}
impl ApprovalStoreRecord {
    pub fn validate(&self) -> Result<()> {
        ensure(
            !self.peer_id.is_empty()
                && self.peer_id.len() <= 200
                && self
                    .peer_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b)),
            "Invalid saved approval peer.",
        )?;
        ensure(
            !self.hostname.is_empty()
                && self.hostname.len() <= 253
                && !self.hostname.chars().any(char::is_control),
            "Invalid saved approval hostname.",
        )?;
        ensure(
            self.approved_at_millis <= 9_007_199_254_740_991,
            "Invalid saved approval timestamp.",
        )
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApprovalStoreSnapshot {
    pub revision: u64,
    pub approvals: Vec<ApprovalStoreRecord>,
}
impl ApprovalStoreSnapshot {
    pub fn validate(&self) -> Result<()> {
        ensure(
            self.revision <= i64::MAX as u64,
            "Invalid approval revision.",
        )?;
        ensure(
            self.approvals.len() <= APPROVAL_RECORD_LIMIT,
            "Too many saved approvals.",
        )?;
        let mut seen = HashSet::new();
        for record in &self.approvals {
            record.validate()?;
            ensure(
                seen.insert(&record.peer_id),
                "Duplicate saved approval peer.",
            )?;
        }
        Ok(())
    }
}
