use crate::{
    validation::{ensure, text},
    Validate,
};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidationError;
impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Invalid Shellfox v1 message.")
    }
}
impl std::error::Error for ValidationError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    Validation,
    NotFound,
    Unsupported,
    DependencyMissing,
    LaunchFailed,
    RegistrationTimeout,
    TargetLost,
    TargetAmbiguous,
    FocusDenied,
    MonitorUnavailable,
    NativeUnavailable,
    AuthFailed,
    ApprovalRequired,
    AccessDenied,
    TerminalControlRequired,
    SettleConfirmRequired,
    RetryConfirmRequired,
    StorageFailed,
    FileConflict,
    Internal,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}
impl Validate for AppError {
    fn validate(&self) -> Result<(), ValidationError> {
        text(&self.message, 1000)?;
        ensure(!self.message.is_empty())
    }
}

/// Uses boolean literals on the wire; malformed success/failure branches are rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply<T> {
    Success(T),
    Failure(AppError),
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, bound(deserialize = "T: Deserialize<'de>"))]
struct RawReply<T> {
    ok: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::validation::present_nullable"
    )]
    value: Option<Option<T>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::validation::present_nullable"
    )]
    error: Option<Option<AppError>>,
}
impl<T: Serialize> Serialize for Reply<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Success(value) => RawReply {
                ok: true,
                value: Some(Some(value)),
                error: None,
            }
            .serialize(serializer),
            Self::Failure(error) => RawReply::<&T> {
                ok: false,
                value: None,
                error: Some(Some(error.clone())),
            }
            .serialize(serializer),
        }
    }
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Reply<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawReply::<T>::deserialize(deserializer)?;
        match (raw.ok, raw.value, raw.error) {
            (true, Some(Some(value)), None) => Ok(Self::Success(value)),
            (false, None, Some(Some(error))) => Ok(Self::Failure(error)),
            _ => Err(serde::de::Error::custom("Invalid result branch.")),
        }
    }
}
impl<T: Validate> Validate for Reply<T> {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Success(value) => value.validate(),
            Self::Failure(error) => error.validate(),
        }
    }
}
pub fn decode<T: DeserializeOwned + Validate>(bytes: &[u8]) -> Result<T, ValidationError> {
    ensure(bytes.len() <= crate::MAX_RESPONSE_BYTES)?;
    let value: T = serde_json::from_slice(bytes).map_err(|_| ValidationError)?;
    value.validate()?;
    Ok(value)
}
