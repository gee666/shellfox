use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    Validation,
    NotFound,
    Unsupported,
    StorageFailed,
    #[serde(rename = "FILE_CONFLICT")]
    Conflict,
    #[serde(rename = "RETRY_CONFIRM_REQUIRED")]
    CleanupRequired,
    Internal,
}

/// Messages deliberately exclude SQL, filesystem paths and external payloads.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[error("{message}")]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}
impl Error {
    pub fn new(code: ErrorCode, message: &str) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: code == ErrorCode::StorageFailed,
        }
    }
}
pub type Result<T> = std::result::Result<T, Error>;
