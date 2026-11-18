use crate::{
    validation::{ensure, text},
    ErrorCode, Validate, ValidationError,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    pub detail: ErrorDetail,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ErrorDetail {
    Application,
    ProfileIncompatible,
    #[serde(rename_all = "camelCase")]
    CoverageUnsupported {
        issues: Vec<ProjectionIssue>,
        issue_count: u32,
    },
    ResourceLimit {
        limit: Limit,
    },
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Limit {
    RequestBytes,
    ResponseBytes,
    RecoveryBytes,
    Inventory,
    Events,
    PendingOperations,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectionIssue {
    pub reason: ProjectionReason,
    pub fields: Vec<FactField>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProjectionReason {
    InvalidMetadata,
    InconsistentMembership,
    EvidenceUnavailable,
    UnsupportedSource,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FactField {
    SourceKind,
    AdapterId,
    ShellId,
    ProfileId,
    CreatedAt,
    Cwd,
    Status,
    Runtime,
    Geometry,
    Recovery,
}
impl Validate for ProjectionIssue {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(!self.fields.is_empty() && self.fields.len() <= 10)
    }
}
impl Error {
    pub fn profile_denied() -> Self {
        Self {
            code: ErrorCode::Unsupported,
            message:
                "observer-v2 grants delivery only, not terminal input, layout or upload authority."
                    .into(),
            retryable: false,
            detail: ErrorDetail::ProfileIncompatible,
        }
    }
}
impl Validate for Error {
    fn validate(&self) -> Result<(), ValidationError> {
        text(&self.message, 1000)?;
        ensure(!self.message.is_empty())?;
        match &self.detail {
            ErrorDetail::Application => Ok(()),
            ErrorDetail::ProfileIncompatible => {
                ensure(self.code == ErrorCode::Unsupported && !self.retryable)
            }
            ErrorDetail::CoverageUnsupported {
                issues,
                issue_count,
            } => {
                ensure(
                    self.code == ErrorCode::Unsupported
                        && !self.retryable
                        && !issues.is_empty()
                        && issues.len() <= 64
                        && *issue_count as usize >= issues.len(),
                )?;
                for issue in issues {
                    issue.validate()?;
                }
                Ok(())
            }
            ErrorDetail::ResourceLimit { .. } => ensure(
                matches!(self.code, ErrorCode::Validation | ErrorCode::Unsupported)
                    && !self.retryable,
            ),
        }
    }
}
