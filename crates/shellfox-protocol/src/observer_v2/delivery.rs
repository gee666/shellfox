use super::*;
use crate::{validation::*, Validate, ValidationError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliveryRef {
    pub tab_id: Id,
    pub generation: Id,
    pub delivery_id: Id,
    pub delivery_epoch: u64,
}
impl Validate for DeliveryRef {
    fn validate(&self) -> Result<(), ValidationError> {
        epoch(self.delivery_epoch)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Resume {
    pub generation: Id,
    pub after_sequence: u64,
}
impl Validate for Resume {
    fn validate(&self) -> Result<(), ValidationError> {
        sequence(self.after_sequence)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttachTerminal {
    pub tab_id: Id,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub resume: Option<Resume>,
}
impl Validate for AttachTerminal {
    fn validate(&self) -> Result<(), ValidationError> {
        if let Some(v) = &self.resume {
            v.validate()?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Recovery {
    pub recovery_id: Id,
    pub format: RecoveryFormat,
    pub encoding: Encoding,
    pub data: String,
    pub last_sequence: u64,
    pub cols: u16,
    pub rows: u16,
    pub layout_revision: u64,
}
impl Validate for Recovery {
    fn validate(&self) -> Result<(), ValidationError> {
        base64_size(&self.data, MAX_RECOVERY_BYTES, false)?;
        // FormattedActiveBufferV1 starts with CAN + RIS, not a continuation of old client state.
        ensure(self.data.starts_with("GBtj"))?;
        sequence(self.last_sequence)?;
        ensure((2..=500).contains(&self.cols) && (2..=500).contains(&self.rows))?;
        epoch(self.layout_revision)
    }
}
impl Recovery {
    pub fn bytes(&self) -> Result<Vec<u8>, ValidationError> {
        self.validate()?;
        decode_bytes(&self.data, MAX_RECOVERY_BYTES)
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Layout {
    FixedCanonicalGrid,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnavailableReason {
    SourceUnavailable,
    FactsUnavailable,
    RecoveryUnavailable,
    GeometryUnsupported,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Attachment {
    Attached {
        delivery: DeliveryRef,
        #[serde(deserialize_with = "required")]
        input: (),
        layout: Layout,
        recovery: Recovery,
    },
    #[serde(rename_all = "camelCase")]
    Unavailable {
        tab_id: Id,
        reason: UnavailableReason,
    },
}
impl Validate for Attachment {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Attached {
                delivery, recovery, ..
            } => {
                delivery.validate()?;
                recovery.validate()
            }
            Self::Unavailable { .. } => Ok(()),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Ack {
    #[serde(rename_all = "camelCase")]
    Recovery {
        recovery_id: Id,
        sequence: u64,
    },
    Data {
        sequence: u64,
    },
}
impl Ack {
    pub fn sequence(&self) -> u64 {
        match self {
            Self::Recovery { sequence, .. } | Self::Data { sequence } => *sequence,
        }
    }
}
impl Validate for Ack {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Recovery { sequence, .. } => sequence_checked(*sequence),
            Self::Data { sequence } => epoch(*sequence),
        }
    }
}
fn sequence_checked(v: u64) -> Result<(), ValidationError> {
    sequence(v)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcknowledgeTerminal {
    pub delivery: DeliveryRef,
    pub ack: Ack,
}
impl Validate for AcknowledgeTerminal {
    fn validate(&self) -> Result<(), ValidationError> {
        self.delivery.validate()?;
        self.ack.validate()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetachTerminal {
    pub delivery: DeliveryRef,
}
impl Validate for DetachTerminal {
    fn validate(&self) -> Result<(), ValidationError> {
        self.delivery.validate()
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RevokedReason {
    ApprovalRevoked,
    ViewDetached,
    SourceUnavailable,
    PolicyChanged,
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
    /// Actual original-process outcome only, never inferred from source EOF or display flags.
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
            Self::Data { data, .. } => base64_size(data, MAX_DATA_BYTES, false),
            _ => Ok(0),
        }
    }
}
impl Validate for Event {
    fn validate(&self) -> Result<(), ValidationError> {
        self.delivery().validate()?;
        match self {
            Self::Data { sequence, data, .. } => {
                epoch(*sequence)?;
                base64_size(data, MAX_DATA_BYTES, false)?;
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
                ..
            } => {
                epoch(*layout_revision)?;
                ensure(*recovery_required && (2..=500).contains(cols) && (2..=500).contains(rows))
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
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundPoll {
    pub version: Version,
    pub binding: Binding,
}
impl Validate for BoundPoll {
    fn validate(&self) -> Result<(), ValidationError> {
        self.binding.validate()
    }
}
