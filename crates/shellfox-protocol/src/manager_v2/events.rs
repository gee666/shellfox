use super::*;
use crate::{validation::*, Validate, ValidationError};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RevokedReason {
    ApprovalRevoked,
    ViewDetached,
    SourceUnavailable,
    PolicyChanged,
    ControlLost,
    ControlReleased,
    GenerationChanged,
    PermissionChanged,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Event {
    Data {
        delivery: DeliveryRef,
        sequence: u64,
        encoding: Encoding,
        data: String,
    },
    #[serde(rename_all = "camelCase")]
    SourceEnded {
        delivery: DeliveryRef,
        last_sequence: u64,
    },
    #[serde(rename_all = "camelCase")]
    ProcessExited {
        delivery: DeliveryRef,
        last_sequence: u64,
        #[serde(deserialize_with = "crate::validation::nullable")]
        exit_code: Option<i64>,
        #[serde(deserialize_with = "crate::validation::nullable")]
        signal: Option<i64>,
    },
    Error {
        delivery: DeliveryRef,
        error: Error,
    },
    Revoked {
        delivery: DeliveryRef,
        reason: RevokedReason,
    },
    #[serde(rename_all = "camelCase")]
    Layout {
        delivery: DeliveryRef,
        layout_revision: u64,
        cols: u16,
        rows: u16,
        recovery_required: bool,
        last_sequence: u64,
    },
}
impl Event {
    pub fn delivery(&self) -> &DeliveryRef {
        match self {
            Self::Data { delivery, .. }
            | Self::SourceEnded { delivery, .. }
            | Self::ProcessExited { delivery, .. }
            | Self::Error { delivery, .. }
            | Self::Revoked { delivery, .. }
            | Self::Layout { delivery, .. } => delivery,
        }
    }
    pub fn decoded_bytes(&self) -> Result<usize, ValidationError> {
        match self {
            Self::Data { data, .. } => {
                ensure(!data.is_empty() && data.len() <= MAX_DATA_BYTES.div_ceil(3) * 4)?;
                let size = crate::chunks::base64_size(data)?;
                ensure(size <= MAX_DATA_BYTES)?;
                Ok(size)
            }
            _ => Ok(0),
        }
    }
}
impl Validate for Event {
    fn validate(&self) -> Result<(), ValidationError> {
        self.delivery().validate()?;
        match self {
            Self::Data { sequence: seq, .. } => {
                epoch(*seq)?;
                self.decoded_bytes()?;
                Ok(())
            }
            Self::SourceEnded { last_sequence, .. } => sequence(*last_sequence),
            Self::ProcessExited {
                last_sequence,
                exit_code,
                signal,
                ..
            } => {
                sequence(*last_sequence)?;
                ensure(exit_code.is_some() || signal.is_some())?;
                for code in [exit_code, signal].into_iter().flatten() {
                    integer(*code)?;
                }
                Ok(())
            }
            Self::Error { error, .. } => error.validate(),
            Self::Revoked { .. } => Ok(()),
            Self::Layout {
                layout_revision,
                cols,
                rows,
                recovery_required,
                last_sequence,
                ..
            } => {
                epoch(*layout_revision)?;
                validate_grid(*cols, *rows)?;
                sequence(*last_sequence)?;
                ensure(*recovery_required)
            }
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum HostEvent {
    Changed { event: crate::ChangedEvent },
    Terminal { event: Event },
}
impl Validate for HostEvent {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Changed { event } => event.validate(),
            Self::Terminal { event } => event.validate(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PollResponse {
    pub events: Vec<HostEvent>,
}
impl Validate for PollResponse {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(self.events.len() <= 1024)?;
        let mut bytes = 0;
        for event in &self.events {
            event.validate()?;
            if let HostEvent::Terminal { event } = event {
                bytes += event.decoded_bytes()?;
            }
        }
        ensure(bytes <= MAX_POLL_DATA_BYTES)
    }
}
