//! Opt-in local-host raw-process RPC. Never a remote manager method.
use crate::{
    validation::{ensure, integer, nullable, optional, sequence},
    LocalRequest, Validate, ValidationError,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

pub const AUTOMATION_RUNS: usize = 64;
pub const AUTOMATION_CHUNK_BYTES: usize = 32768;
/// Active wire allows 65536 canonical base64 characters. CLI sends 32768 bytes.
pub const AUTOMATION_INPUT_BYTES: usize = 49152;
pub const AUTOMATION_CREDIT: usize = 1024 * 1024;
pub const AUTOMATION_REPLAY: usize = 256 * 1024;

// No Debug: these contain caller arguments, environment and paths.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationStart {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub request_id: Uuid,
    pub executable: String,
    pub args: Vec<String>,
    pub cwd: String,
    #[serde(deserialize_with = "environment")]
    pub env: BTreeMap<String, String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional"
    )]
    pub title: Option<String>,
    pub detached: bool,
}
impl Validate for AutomationStart {
    fn validate(&self) -> Result<(), ValidationError> {
        nonempty(&self.executable)?;
        nonempty(&self.cwd)?;
        ensure(self.args.len() <= 4096 && self.env.len() <= 1024)?;
        for arg in &self.args {
            text(arg)?;
        }
        for (key, value) in &self.env {
            ensure(
                !key.is_empty() && key.encode_utf16().count() <= 1024 && !key.contains(['=', '\0']),
            )?;
            text(value)?;
        }
        if let Some(title) = &self.title {
            ensure(!title.is_empty() && title.encode_utf16().count() <= 200)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationRun {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub id: Uuid,
    pub pid: u32,
    pub executable: String,
    pub cwd: String,
    pub title: String,
    pub state: AutomationState,
    #[serde(deserialize_with = "nullable")]
    pub exit_code: Option<i64>,
    #[serde(deserialize_with = "nullable")]
    pub signal: Option<String>,
    #[serde(deserialize_with = "nullable_id")]
    pub session_id: Option<Uuid>,
    #[serde(deserialize_with = "nullable_id")]
    pub tab_id: Option<Uuid>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutomationState {
    Running,
    Exited,
}
impl Validate for AutomationRun {
    fn validate(&self) -> Result<(), ValidationError> {
        text(&self.executable)?;
        text(&self.cwd)?;
        ensure(!self.title.is_empty() && self.title.encode_utf16().count() <= 200)?;
        if let Some(code) = self.exit_code {
            integer(code)?;
        }
        if let Some(signal) = &self.signal {
            text(signal)?;
        }
        ensure(self.session_id.is_some() == self.tab_id.is_some())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutomationStream {
    Stdout,
    Stderr,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationFrame {
    pub sequence: u64,
    pub stream: AutomationStream,
    pub data: String,
}
impl Validate for AutomationFrame {
    fn validate(&self) -> Result<(), ValidationError> {
        sequence(self.sequence)?;
        ensure(self.sequence > 0)?;
        bytes(&self.data)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationRead {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub id: Uuid,
    pub after: u64,
}
impl Validate for AutomationRead {
    fn validate(&self) -> Result<(), ValidationError> {
        sequence(self.after)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationOutput {
    pub frames: Vec<AutomationFrame>,
    pub run: AutomationRun,
}
impl Validate for AutomationOutput {
    fn validate(&self) -> Result<(), ValidationError> {
        self.run.validate()?;
        ensure(self.frames.len() <= 32)?;
        let mut previous = 0;
        for frame in &self.frames {
            frame.validate()?;
            ensure(frame.sequence > previous)?;
            previous = frame.sequence;
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationWrite {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub id: Uuid,
    pub data: String,
    pub eof: bool,
}
impl Validate for AutomationWrite {
    fn validate(&self) -> Result<(), ValidationError> {
        bytes(&self.data)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationList {}
impl Validate for AutomationList {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AutomationListing(pub Vec<AutomationRun>);
impl Validate for AutomationListing {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.0.len() <= AUTOMATION_RUNS)?;
        for run in &self.0 {
            run.validate()?;
        }
        Ok(())
    }
}
macro_rules! identity {
    ($ty:ident) => {
        #[derive(Clone, Debug, Serialize, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $ty {
            #[serde(deserialize_with = "crate::ids::wire_id")]
            pub id: Uuid,
        }
        impl Validate for $ty {
            fn validate(&self) -> Result<(), ValidationError> {
                Ok(())
            }
        }
    };
}
identity!(AutomationStop);
identity!(AutomationShow);
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationAccepted {
    pub accepted: bool,
}
impl Validate for AutomationAccepted {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.accepted)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutomationStopped {
    pub stopped: bool,
}
impl Validate for AutomationStopped {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.stopped)
    }
}
macro_rules! method {
    ($request:ty, $response:ty, $method:literal) => {
        impl LocalRequest for $request {
            type Response = $response;
            const METHOD: &'static str = $method;
        }
    };
}
method!(AutomationStart, AutomationRun, "automation.start");
method!(AutomationList, AutomationListing, "automation.list");
method!(AutomationRead, AutomationOutput, "automation.read");
method!(AutomationWrite, AutomationAccepted, "automation.write");
method!(AutomationStop, AutomationStopped, "automation.stop");
method!(AutomationShow, AutomationRun, "automation.show");
fn nullable_id<'de, D: serde::Deserializer<'de>>(de: D) -> Result<Option<Uuid>, D::Error> {
    let value = Option::<String>::deserialize(de)?;
    value
        .map(|value| {
            if !crate::ids::is_wire_id(&value) {
                return Err(serde::de::Error::custom("Expected an RFC UUID."));
            }
            value
                .parse()
                .map_err(|_| serde::de::Error::custom("Expected a UUID."))
        })
        .transpose()
}
fn text(value: &str) -> Result<(), ValidationError> {
    ensure(value.encode_utf16().count() <= 32768 && !value.contains('\0'))
}
fn nonempty(value: &str) -> Result<(), ValidationError> {
    text(value)?;
    ensure(!value.is_empty())
}
fn bytes(data: &str) -> Result<(), ValidationError> {
    ensure(data.len() <= 65536)?;
    ensure(crate::chunks::base64_size(data)? <= AUTOMATION_INPUT_BYTES)
}
fn environment<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    struct Entries;
    impl<'de> serde::de::Visitor<'de> for Entries {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("unique environment entries")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut entries = BTreeMap::new();
            while let Some((key, value)) = map.next_entry::<String, String>()? {
                if entries.len() >= 1024 || entries.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom(
                        "Duplicate or oversized environment.",
                    ));
                }
            }
            Ok(entries)
        }
    }
    de.deserialize_map(Entries)
}
