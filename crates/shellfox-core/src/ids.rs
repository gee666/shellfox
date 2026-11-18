use crate::{Error, ErrorCode, Result};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use uuid::Uuid;

macro_rules! id {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize,
        )]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(Uuid);
        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }
        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
        impl FromStr for $name {
            type Err = Error;
            fn from_str(value: &str) -> Result<Self> {
                let parsed = Uuid::parse_str(value)
                    .map_err(|_| Error::new(ErrorCode::Validation, "Expected a UUID."))?;
                if !parsed.to_string().eq_ignore_ascii_case(value) {
                    return Err(Error::new(
                        ErrorCode::Validation,
                        "Expected a hyphenated UUID.",
                    ));
                }
                Ok(Self(parsed))
            }
        }
        impl TryFrom<String> for $name {
            type Error = Error;
            fn try_from(value: String) -> Result<Self> {
                value.parse()
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> String {
                value.to_string()
            }
        }
    };
}
id!(SessionId);
id!(TabId);
id!(Generation);
id!(CleanupToken);

/// Opaque profile metadata, not permission to execute a path or command.
/// Availability and executable selection belong to the trusted terminal adapter.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProfileId(String);
impl ProfileId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        crate::settings_validation::profile(&value)?;
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl Default for ProfileId {
    fn default() -> Self {
        Self("login-shell".into())
    }
}
impl FromStr for ProfileId {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        Self::new(value)
    }
}
impl TryFrom<String> for ProfileId {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        Self::new(value)
    }
}
impl From<ProfileId> for String {
    fn from(value: ProfileId) -> String {
        value.0
    }
}
impl fmt::Display for ProfileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
