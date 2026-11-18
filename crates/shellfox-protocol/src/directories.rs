use crate::{methods::method, validation::*, *};
use serde::{Deserialize, Serialize};
pub const DIRECTORY_COMPLETION_LIMIT: usize = 100;
pub const DIRECTORY_COMPLETION_BYTES: usize = 65536;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolveDirectory {
    pub path: String,
}
impl Validate for ResolveDirectory {
    fn validate(&self) -> Result<(), ValidationError> {
        text(&self.path, 32767)
    }
}
method!(ResolveDirectory, Directory, "resolveDirectory");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateDirectory {
    pub path: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::ids::optional_wire_id"
    )]
    pub session_id: Option<SessionId>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::ids::optional_wire_id"
    )]
    pub tab_id: Option<TabId>,
}
impl Validate for CreateDirectory {
    fn validate(&self) -> Result<(), ValidationError> {
        text(&self.path, 32767)?;
        ensure(self.tab_id.is_none() || self.session_id.is_some())
    }
}
method!(CreateDirectory, Directory, "createDirectory");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompleteDirectory {
    pub path: String,
}
impl Validate for CompleteDirectory {
    fn validate(&self) -> Result<(), ValidationError> {
        text(&self.path, 32767)
    }
}
method!(CompleteDirectory, DirectoryMatches, "completeDirectory");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryMatches {
    pub matches: Vec<String>,
}
impl Validate for DirectoryMatches {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.matches.len() <= DIRECTORY_COMPLETION_LIMIT)?;
        let mut bytes = 0;
        for value in &self.matches {
            text(value, 32767)?;
            ensure(!value.is_empty())?;
            bytes += value.len();
        }
        ensure(bytes <= DIRECTORY_COMPLETION_BYTES)
    }
}
