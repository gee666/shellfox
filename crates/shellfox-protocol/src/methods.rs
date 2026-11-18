use crate::{validation::*, *};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// Implementations are sealed to the verified remote allowlist.
pub trait Request: private::Sealed + Serialize + DeserializeOwned + Validate {
    type Response: DeserializeOwned + Serialize + Validate;
    const METHOD: &'static str;
    const LEGACY_UPLOAD: bool = false;
    fn validate_wire(&self) -> Result<(), ValidationError> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| ValidationError)?;
        ensure(bytes.len() <= MAX_REQUEST_BYTES)?;
        // Core UUIDs accept shorthand; the wire deserializers enforce the TS UUID grammar.
        let _: Self = serde_json::from_slice(&bytes).map_err(|_| ValidationError)?;
        Ok(())
    }
}
pub(crate) mod private {
    pub trait Sealed {}
}
macro_rules! method {
    ($ty:ty, $response:ty, $name:literal) => {
        impl crate::methods::private::Sealed for $ty {}
        impl crate::Request for $ty {
            type Response = $response;
            const METHOD: &'static str = $name;
        }
    };
}
pub(crate) use method;

macro_rules! empty {
    ($ty:ident, $response:ty, $name:literal) => {
        #[derive(Clone, Debug, Default, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $ty {}
        impl Validate for $ty {
            fn validate(&self) -> Result<(), ValidationError> {
                Ok(())
            }
        }
        method!($ty, $response, $name);
    };
}
empty!(GetSnapshot, ManagerSnapshot, "getSnapshot");
empty!(GetTerminalProfiles, TerminalProfiles, "getTerminalProfiles");
empty!(GetHomeDirectory, Directory, "getHomeDirectory");

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Directory {
    pub cwd: String,
}
impl Validate for Directory {
    fn validate(&self) -> Result<(), ValidationError> {
        cwd(&self.cwd)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListFiles {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::ids::optional_wire_id")]
    pub session_id: Option<SessionId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::ids::optional_wire_id")]
    pub tab_id: Option<TabId>,
    pub path: String,
}
impl Validate for ListFiles {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.tab_id.is_none() || self.session_id.is_some())?;
        files::path(&self.path)
    }
}
method!(ListFiles, files::FileList, "listFiles");
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::ids::optional_wire_id")]
    pub session_id: Option<SessionId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::ids::optional_wire_id")]
    pub tab_id: Option<TabId>,
    pub path: String,
}
impl Validate for ReadFile {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.tab_id.is_none() || self.session_id.is_some())?;
        files::path(&self.path)
    }
}
method!(ReadFile, files::FileRead, "readFile");
method!(files::WriteFile, files::FileWriteResult, "writeFile");

/// The v1 remote envelope has no manager RPC version field.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestEnvelope<'a, R: Request> {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub client_id: ClientId,
    pub method: &'static str,
    pub payload: &'a R,
}
impl<'a, R: Request> RequestEnvelope<'a, R> {
    pub fn new(client_id: ClientId, payload: &'a R) -> Self {
        Self {
            client_id,
            method: R::METHOD,
            payload,
        }
    }
}
