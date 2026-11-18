//! Explicit manager-v2 types. These declarations do not enable a production route.
mod command;
mod control;
mod errors;
mod events;
mod handshake;
mod input;
mod key_v2;
mod mouse;
mod physical;
mod recovery;
mod requests;
mod uploads;
pub use crate::observer_v2::{
    CatalogEntry, CatalogMode, DeliveryFormat, DeliveryRef, Disconnected, Discover, Discovery,
    Encoding, Fact, FactField, Id, ProjectionIssue, ProjectionReason, Resume, Session,
    SessionFacts, SessionStats, Settings, Snapshot, SourceKind, Tab, TabFacts, UnavailableReason,
    UnknownReason, Version, MAX_DATA_BYTES, MAX_POLL_DATA_BYTES, MAX_RECOVERY_BYTES,
};
pub use command::*;
pub use control::*;
pub use errors::*;
pub use events::*;
pub use handshake::*;
pub use input::*;
pub use key_v2::*;
pub use mouse::*;
pub use physical::*;
pub use recovery::*;
pub use requests::*;
pub use uploads::*;
pub const PROFILE: &str = "manager-v2";

/// Claimed build identity from authenticated negotiation, not binary authenticity.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct EngineIdentity(String);
impl EngineIdentity {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for EngineIdentity {
    type Error = crate::ValidationError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(crate::ValidationError);
        }
        Ok(Self(value))
    }
}
impl From<EngineIdentity> for String {
    fn from(value: EngineIdentity) -> Self {
        value.0
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RecoveryFormat {
    #[default]
    #[serde(rename = "formatted-active-buffer-v1")]
    FormattedActiveBuffer,
    #[serde(rename = "native0-raw-v1")]
    Native0RawV1,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Native0Codec {
    pub engine_identity: EngineIdentity,
    pub extension_abi: u32,
    pub terminal_profile_abi: u32,
    pub snapshot_budget_abi: u32,
    pub wire_profile: u32,
    pub library_version: u32,
    pub snapshot_version: u32,
    pub terminal_profile: u32,
}
impl Native0Codec {
    pub fn new(engine_identity: EngineIdentity) -> Self {
        Self {
            engine_identity,
            extension_abi: 1,
            terminal_profile_abi: 1,
            snapshot_budget_abi: 1,
            wire_profile: 1,
            library_version: 0x00010001,
            snapshot_version: 1,
            terminal_profile: 0,
        }
    }
}
impl crate::Validate for Native0Codec {
    fn validate(&self) -> Result<(), crate::ValidationError> {
        crate::validation::ensure(
            self.extension_abi == 1
                && self.terminal_profile_abi == 1
                && self.snapshot_budget_abi == 1
                && self.wire_profile == 1
                && self.library_version == 0x00010001
                && self.snapshot_version == 1
                && self.terminal_profile == 0,
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Native0Configuration {
    pub runtime_image_storage_limit: u64,
    pub runtime_resize_pull_scrollback: bool,
}
impl crate::Validate for Native0Configuration {
    fn validate(&self) -> Result<(), crate::ValidationError> {
        crate::validation::ensure(self.runtime_image_storage_limit <= crate::MAX_SAFE_INTEGER)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Native0State {
    pub abi_version: u32,
    pub terminal_profile: u32,
    pub preceding_join: u32,
}
impl crate::Validate for Native0State {
    fn validate(&self) -> Result<(), crate::ValidationError> {
        crate::validation::ensure(
            self.abi_version == 1 && self.terminal_profile == 0 && self.preceding_join == 0,
        )
    }
}
/// Finite sender accounting limits. A receiver applies its own limits as well.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Native0Limits {
    pub max_encoded_bytes: u32,
    pub max_allocation_bytes: u64,
    pub max_work_units: u64,
    pub max_cells: u64,
    pub max_history_rows: u64,
    pub max_continuation_bytes: u32,
    pub max_columns: u32,
    pub max_rows: u32,
    pub max_pixel_width: u32,
    pub max_pixel_height: u32,
    pub max_page_rows: u32,
    pub max_pages: u32,
    pub max_records: u32,
}
impl crate::Validate for Native0Limits {
    fn validate(&self) -> Result<(), crate::ValidationError> {
        crate::validation::ensure(
            self.max_encoded_bytes as usize <= MAX_RECOVERY_BYTES
                && self.max_allocation_bytes <= 512 * 1024 * 1024
                && self.max_work_units <= 100_000_000_000
                && self.max_cells <= 16 * 1024 * 1024
                && self.max_history_rows <= 1_048_576
                && self.max_continuation_bytes <= 65536
                && self.max_columns <= 4096
                && self.max_rows <= 4096
                && self.max_pixel_width <= 65536
                && self.max_pixel_height <= 65536
                && self.max_page_rows <= 65535
                && self.max_pages <= 65536
                && self.max_records <= 1_048_576,
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Native0Metadata {
    pub codec: Native0Codec,
    pub state: Native0State,
    pub configuration: Native0Configuration,
    pub limits: Native0Limits,
}
impl crate::Validate for Native0Metadata {
    fn validate(&self) -> Result<(), crate::ValidationError> {
        self.codec.validate()?;
        self.state.validate()?;
        self.configuration.validate()?;
        self.limits.validate()
    }
}
