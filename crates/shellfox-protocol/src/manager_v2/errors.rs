use super::*;
use crate::{validation::*, ErrorCode, Validate, ValidationError};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Limit {
    RequestBytes,
    ResponseBytes,
    RecoveryBytes,
    Inventory,
    Events,
    PendingOperations,
    InputPayloadBytes,
    EncodedInputBytes,
}
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
    #[serde(rename_all = "camelCase")]
    StaleLayout {
        recovery_required: bool,
    },
    #[serde(rename_all = "camelCase")]
    InputOutcomeUnknown {
        input_id: Id,
        retry_input: bool,
    },
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
            ErrorDetail::StaleLayout { recovery_required } => {
                ensure(*recovery_required && !self.retryable && self.code == ErrorCode::TargetLost)
            }
            ErrorDetail::InputOutcomeUnknown { retry_input, .. } => {
                ensure(!retry_input && !self.retryable && self.code == ErrorCode::NativeUnavailable)
            }
        }
    }
}
pub type Reply<T> = crate::v2_replies::Reply<T, Error>;
pub type BoundReply<T> = crate::v2_replies::BoundReply<T, Binding, Error>;
