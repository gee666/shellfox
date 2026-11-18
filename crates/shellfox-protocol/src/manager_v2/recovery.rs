use super::*;
use crate::{validation::*, Validate, ValidationError};
use serde::{Deserialize, Serialize};
/// Distinct typed payloads. Native0 bytes are never parsed/labelled as ANSI.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "format", deny_unknown_fields)]
pub enum Recovery {
    #[serde(rename = "formatted-active-buffer-v1", rename_all = "camelCase")]
    FormattedActiveBuffer {
        recovery_id: Id,
        encoding: Encoding,
        data: String,
        last_sequence: u64,
        cols: u16,
        rows: u16,
        layout_revision: u64,
    },
    #[serde(rename = "native0-raw-v1", rename_all = "camelCase")]
    Native0RawV1 {
        recovery_id: Id,
        encoding: Encoding,
        data: String,
        last_sequence: u64,
        cols: u16,
        rows: u16,
        layout_revision: u64,
        metadata: Native0Metadata,
    },
}
impl Recovery {
    fn fields(&self) -> (Id, &str, u64, u16, u16, u64) {
        match self {
            Self::FormattedActiveBuffer {
                recovery_id,
                data,
                last_sequence,
                cols,
                rows,
                layout_revision,
                ..
            }
            | Self::Native0RawV1 {
                recovery_id,
                data,
                last_sequence,
                cols,
                rows,
                layout_revision,
                ..
            } => (
                *recovery_id,
                data,
                *last_sequence,
                *cols,
                *rows,
                *layout_revision,
            ),
        }
    }
    pub fn recovery_id(&self) -> Id {
        self.fields().0
    }
    pub fn last_sequence(&self) -> u64 {
        self.fields().2
    }
    pub fn cols(&self) -> u16 {
        self.fields().3
    }
    pub fn rows(&self) -> u16 {
        self.fields().4
    }
    pub fn layout_revision(&self) -> u64 {
        self.fields().5
    }
    pub fn format(&self) -> RecoveryFormat {
        match self {
            Self::FormattedActiveBuffer { .. } => RecoveryFormat::FormattedActiveBuffer,
            Self::Native0RawV1 { .. } => RecoveryFormat::Native0RawV1,
        }
    }
    pub fn bytes(&self) -> Result<Vec<u8>, ValidationError> {
        self.validate()?;
        crate::observer_v2::decode_bytes(self.fields().1, MAX_RECOVERY_BYTES)
    }
    /// Expected sender is the retained authenticated PAIR selection, not this body.
    pub fn check_selection(&self, selection: &PairSelection) -> Result<(), ValidationError> {
        self.validate()?;
        selection.validate()?;
        ensure(self.format() == selection.recovery_format)?;
        match self {
            Self::Native0RawV1 { metadata, .. } => {
                ensure(selection.native0_sender.as_ref() == Some(&metadata.codec))
            }
            Self::FormattedActiveBuffer { .. } => Ok(()),
        }
    }
}
impl Validate for Recovery {
    fn validate(&self) -> Result<(), ValidationError> {
        let (_, data, seq, cols, rows, revision) = self.fields();
        ensure(!data.is_empty() && data.len() <= MAX_RECOVERY_BYTES.div_ceil(3) * 4)?;
        ensure(crate::chunks::base64_size(data)? <= MAX_RECOVERY_BYTES)?;
        match self {
            Self::FormattedActiveBuffer { .. } => ensure(data.starts_with("GBtj"))?,
            Self::Native0RawV1 { metadata, .. } => {
                metadata.validate()?;
                ensure(
                    crate::chunks::base64_size(data)? <= metadata.limits.max_encoded_bytes as usize,
                )?;
            }
        }
        sequence(seq)?;
        validate_grid(cols, rows)?;
        epoch(revision)?;
        // Measure real JSON encoding, not decoded bytes alone. The final bound
        // reply is also checked by the existing carrier before any send.
        ensure(
            serde_json::to_vec(self).map_err(|_| ValidationError)?.len()
                <= crate::MAX_RESPONSE_BYTES,
        )
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Ack {
    #[serde(rename_all = "camelCase")]
    Recovery {
        recovery_id: Id,
        sequence: u64,
        layout_revision: u64,
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
            Self::Recovery {
                sequence: seq,
                layout_revision,
                ..
            } => {
                sequence(*seq)?;
                epoch(*layout_revision)
            }
            Self::Data { sequence: seq } => epoch(*seq),
        }
    }
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
