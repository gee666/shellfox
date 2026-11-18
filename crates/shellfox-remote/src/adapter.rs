//! Pure v1 projections. Metadata intent never proves that a terminal is alive.
use serde::{de::DeserializeOwned, Serialize};
use shellfox_core as core;
use shellfox_protocol as wire;
use std::collections::HashMap;

#[path = "adapter_manager.rs"]
mod manager;
#[path = "adapter_settings.rs"]
mod settings;
#[path = "adapter_terminal.rs"]
mod terminal;
pub use manager::*;
pub use settings::*;
pub use terminal::*;

pub type AdapterResult<T> = Result<T, AdapterError>;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AdapterError {
    #[error("Capability unavailable: {capability}.")]
    CapabilityUnavailable { capability: &'static str },
    #[error("Invalid adapter facts: {context}.")]
    InvalidFacts { context: &'static str },
}
impl From<AdapterError> for wire::AppError {
    fn from(error: AdapterError) -> Self {
        Self {
            code: match error {
                AdapterError::CapabilityUnavailable { .. } => wire::ErrorCode::Unsupported,
                AdapterError::InvalidFacts { .. } => wire::ErrorCode::Validation,
            },
            message: error.to_string(),
            retryable: false,
        }
    }
}
fn required<T>(value: Option<T>, capability: &'static str) -> AdapterResult<T> {
    value.ok_or(AdapterError::CapabilityUnavailable { capability })
}
fn ensure(valid: bool, context: &'static str) -> AdapterResult<()> {
    if valid {
        Ok(())
    } else {
        Err(AdapterError::InvalidFacts { context })
    }
}
fn safe_integer(value: Option<i64>) -> AdapterResult<()> {
    ensure(
        value.is_none_or(|value| value.unsigned_abs() <= wire::MAX_SAFE_INTEGER),
        "integer exceeds JavaScript safe range",
    )
}
// Chrono also accepts lowercase/space separators and leap seconds; Zod does not.
fn wire_timestamp(value: &str) -> AdapterResult<()> {
    let bytes = value.as_bytes();
    ensure(bytes.len() >= 20, "wire timestamp")?;
    ensure(
        [4, 7].into_iter().all(|i| bytes[i] == b'-')
            && bytes[10] == b'T'
            && [13, 16].into_iter().all(|i| bytes[i] == b':')
            && bytes.last() == Some(&b'Z')
            && [0..4, 5..7, 8..10, 11..13, 14..16, 17..19]
                .into_iter()
                .all(|range| bytes[range].iter().all(u8::is_ascii_digit))
            && bytes[17] <= b'5'
            && (bytes.len() == 20
                || bytes.len() > 21
                    && bytes[19] == b'.'
                    && bytes[20..bytes.len() - 1].iter().all(u8::is_ascii_digit)),
        "wire timestamp",
    )?;
    wire::validation::timestamp(value).map_err(|_| AdapterError::InvalidFacts {
        context: "wire timestamp",
    })
}
fn optional_wire_timestamp(value: Option<&str>) -> AdapterResult<()> {
    value.map_or(Ok(()), wire_timestamp)
}
fn wire_identity(value: impl std::fmt::Display) -> AdapterResult<()> {
    serde_json::from_value::<wire::ViewIdentity>(serde_json::json!({
        "clientId": value.to_string()
    }))
    .map(|_| ())
    .map_err(|_| AdapterError::InvalidFacts {
        context: "wire UUID",
    })
}
/// Round-trip checks required fields and serde's wire-only constraints.
fn checked<T>(value: T, context: &'static str) -> AdapterResult<T>
where
    T: Serialize + DeserializeOwned + wire::Validate,
{
    let bytes = serde_json::to_vec(&value).map_err(|_| AdapterError::InvalidFacts { context })?;
    wire::decode::<T>(&bytes).map_err(|_| AdapterError::InvalidFacts { context })?;
    Ok(value)
}

/// Absence means the service cannot answer this part of the legacy contract.
/// Explicit unsupported integration DTOs are valid evidence; omitted ones are not.
#[derive(Clone, Debug, Default)]
pub struct ManagerFacts {
    pub settings: Option<SettingsFacts>,
    pub probe: Option<wire::NativeProbe>,
    pub explorer: Option<wire::ExplorerIntegration>,
    pub cli: Option<wire::CliIntegration>,
    pub sessions: HashMap<core::SessionId, SessionFacts>,
}
#[derive(Clone, Debug)]
pub struct SettingsFacts {
    pub python_path: Option<String>,
    pub adapter_id: wire::AdapterId,
    pub shell_id: wire::ShellId,
    pub shell_executable: Option<String>,
    pub process_rules: Vec<wire::ProcessRule>,
    pub history_page_size: u32,
    pub session_settle_hours: f64,
    pub session_delete_hours: f64,
    pub auto_update: bool,
    pub terminal_env: Vec<core::EnvVar>,
}
#[derive(Clone, Debug)]
pub struct SessionFacts {
    pub adapter_id: wire::AdapterId,
    pub shell_id: wire::ShellId,
    pub window: Option<wire::SessionWindow>,
    pub can_focus: bool,
    pub can_add_tab: bool,
    pub control_reason: Option<String>,
    pub error: Option<wire::AppError>,
    pub terminal_lifetime: Option<wire::TerminalLifetime>,
    pub shell_survival: Option<bool>,
    pub tabs: HashMap<core::TabId, TabFacts>,
}
#[derive(Clone, Debug)]
pub struct TabFacts {
    /// Core tabs have no creation timestamp. Do not substitute the session's.
    pub created_at: String,
    pub lifecycle: wire::TabLifecycle,
    pub observation: ObservationFacts,
    pub error: Option<wire::AppError>,
    pub generation: Option<core::Generation>,
    pub terminal_kind: Option<wire::TerminalKind>,
    pub exit_code: Option<Option<i64>>,
}
#[derive(Clone, Debug)]
pub enum ObservationFacts {
    Unavailable {
        reason: String,
    },
    Exited,
    Alive {
        agents: u64,
        healthy: bool,
        reason: Option<String>,
    },
}

pub fn changed_event(event: &core::ChangedEvent) -> AdapterResult<wire::HostEvent> {
    if let Some(id) = event.select_session_id {
        wire_identity(id)?;
    }
    checked(
        wire::HostEvent::Changed {
            event: wire::ChangedEvent {
                revision: event.revision,
                reason: match event.reason {
                    core::ChangeReason::Sessions => wire::ChangeReason::Sessions,
                    core::ChangeReason::Settings => wire::ChangeReason::Settings,
                },
                select_session_id: event.select_session_id,
            },
        },
        "changed event",
    )
}
