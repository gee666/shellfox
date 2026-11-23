//! Private host power requests, not remote Manager commands or mobile screen wake.
use serde::{Deserialize, Serialize};
use shellfox_protocol::{LocalRequest, Validate, ValidationError};

pub const MAX_POWER_MESSAGE_BYTES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "camelCase", deny_unknown_fields)]
pub enum PowerRequest {
    Status {},
    Set { active: bool },
}
impl Validate for PowerRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
impl LocalRequest for PowerRequest {
    type Response = PowerReport;
    const METHOD: &'static str = shellfox_protocol::POWER_NAMESPACE;
}

/// Confirmed state, never an optimistic toggle. Check `pending` and `error`
/// before treating a set as successful. Failed release retains its original lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PowerReport {
    pub status: super::PowerStatus,
    pub partial_reason: Option<String>,
    pub error: Option<String>,
    pub pending: bool,
}
impl Validate for PowerReport {
    fn validate(&self) -> Result<(), ValidationError> {
        for text in [&self.partial_reason, &self.error].into_iter().flatten() {
            shellfox_protocol::validation::ensure(
                !text.is_empty() && text.len() <= MAX_POWER_MESSAGE_BYTES,
            )?;
        }
        shellfox_protocol::validation::ensure(
            self.status != super::PowerStatus::Partial || self.partial_reason.is_some(),
        )?;
        shellfox_protocol::validation::ensure(
            self.status != super::PowerStatus::ReleaseFailed || self.error.is_some(),
        )
    }
}
pub(super) fn bounded(text: impl AsRef<str>) -> String {
    let text = text.as_ref();
    let mut end = text.len().min(MAX_POWER_MESSAGE_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}
