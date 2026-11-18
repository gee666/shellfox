use serde::{Deserialize, Serialize};
use std::fmt;

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
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
pub type HostError = Error;

/// Existing Shellfox result shape, not Rust's externally tagged Result encoding.
#[derive(Clone, Debug)]
pub struct Reply<T>(Result<T>);
impl<T> Reply<T> {
    pub fn into_result(self) -> Result<T> {
        self.0
    }
    pub fn is_ok(&self) -> bool {
        self.0.is_ok()
    }
}
impl<T> From<Result<T>> for Reply<T> {
    fn from(result: Result<T>) -> Self {
        Self(result)
    }
}
impl<T: Serialize> Serialize for Reply<T> {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Result", 2)?;
        match &self.0 {
            Ok(value) => {
                state.serialize_field("ok", &true)?;
                state.serialize_field("value", value)?;
            }
            Err(error) => {
                state.serialize_field("ok", &false)?;
                state.serialize_field("error", error)?;
            }
        }
        state.end()
    }
}
impl<'de, T: serde::de::DeserializeOwned> Deserialize<'de> for Reply<T> {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        use serde::de::Error as _;
        let raw = serde_json::Value::deserialize(deserializer)?;
        let object = raw
            .as_object()
            .ok_or_else(|| D::Error::custom("Expected a result object"))?;
        if object.len() != 2 {
            return Err(D::Error::custom("Invalid result fields"));
        }
        match object.get("ok").and_then(serde_json::Value::as_bool) {
            Some(true) => {
                let value = object
                    .get("value")
                    .ok_or_else(|| D::Error::custom("Missing result value"))?;
                Ok(Self(Ok(
                    serde_json::from_value(value.clone()).map_err(D::Error::custom)?
                )))
            }
            Some(false) => {
                let value = object
                    .get("error")
                    .ok_or_else(|| D::Error::custom("Missing result error"))?;
                let error: Error =
                    serde_json::from_value(value.clone()).map_err(D::Error::custom)?;
                if error.message.is_empty() || error.message.len() > 1000 {
                    return Err(D::Error::custom("Invalid error message"));
                }
                Ok(Self(Err(error)))
            }
            None => Err(D::Error::custom("Invalid result discriminant")),
        }
    }
}
