use super::primitives::required;
use crate::{Validate, ValidationError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    deny_unknown_fields,
    bound(deserialize = "T: Deserialize<'de>")
)]
pub enum Fact<T> {
    Known {
        #[serde(deserialize_with = "required")]
        value: T,
    },
    Unknown {
        reason: UnknownReason,
    },
    NotApplicable,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnknownReason {
    NotRecorded,
    Pending,
    Unavailable,
    InvalidEvidence,
}
impl<T> Fact<T> {
    pub fn check(
        &self,
        check: impl FnOnce(&T) -> Result<(), ValidationError>,
    ) -> Result<(), ValidationError> {
        match self {
            Self::Known { value } => check(value),
            _ => Ok(()),
        }
    }
    pub fn known(&self) -> Option<&T> {
        match self {
            Self::Known { value } => Some(value),
            _ => None,
        }
    }
}
impl<T: Validate> Validate for Fact<T> {
    fn validate(&self) -> Result<(), ValidationError> {
        self.check(Validate::validate)
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    Pty,
    AutomationPipe,
    ExternalBytes,
}
