use super::*;
use crate::{validation::*, Validate, ValidationError};
use serde::{Deserialize, Serialize};
/// Decoded text/key/paste payload bound, before owner encoding.
pub const MAX_INPUT_PAYLOAD_BYTES: usize = 65536;
/// Actual complete encoded write bound. Owner expansion may reject a valid payload before I/O.
pub const MAX_ENCODED_INPUT_BYTES: usize = 65536;
/// Compatibility name for existing M2 consumers; not a promise that every payload fits after encoding.
pub const MAX_INPUT_BYTES: usize = MAX_INPUT_PAYLOAD_BYTES;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputHints {
    pub user_initiated: bool,
    pub activity_hint: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present_nullable"
    )]
    pub settled_at: Option<Option<String>>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettlementCapture<'a> {
    Unavailable,
    Captured(Option<&'a str>),
}
impl InputHints {
    pub fn settlement_capture(&self) -> SettlementCapture<'_> {
        match &self.settled_at {
            None => SettlementCapture::Unavailable,
            Some(value) => SettlementCapture::Captured(value.as_deref()),
        }
    }
}
impl Validate for InputHints {
    fn validate(&self) -> Result<(), ValidationError> {
        if let Some(value) = &self.settled_at {
            optional_time(value)?;
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BytesOrigin {
    Key,
    Paste,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Input {
    Key {
        event: KeyEventV2,
    },
    Paste {
        text: String,
    },
    Bytes {
        encoding: Encoding,
        data: String,
        origin: BytesOrigin,
    },
    Mouse {
        event: MouseEvent,
        geometry: GeometryStamp,
    },
}
impl Input {
    pub fn kind(&self) -> InputKind {
        match self {
            Self::Key { .. } => InputKind::Key,
            Self::Paste { .. } => InputKind::Paste,
            Self::Bytes { .. } => InputKind::Bytes,
            Self::Mouse { .. } => InputKind::Mouse,
        }
    }
    pub fn decoded_bytes(&self) -> Result<Option<Vec<u8>>, ValidationError> {
        match self {
            Self::Bytes { data, .. } => {
                self.validate()?;
                crate::observer_v2::decode_bytes(data, MAX_INPUT_BYTES).map(Some)
            }
            _ => Ok(None),
        }
    }
}
impl Validate for Input {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Key { event } => event.validate(),
            Self::Paste { text } => {
                ensure(!text.is_empty() && text.len() <= MAX_INPUT_PAYLOAD_BYTES)
            }
            Self::Bytes { data, .. } => {
                ensure(!data.is_empty() && data.len() <= MAX_ENCODED_INPUT_BYTES.div_ceil(3) * 4)?;
                ensure(crate::chunks::base64_size(data)? <= MAX_ENCODED_INPUT_BYTES)
            }
            Self::Mouse { event, geometry } => {
                event.validate()?;
                geometry.validate()
            }
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteTerminal {
    pub control: ControlRef,
    pub input_id: Id,
    pub hints: InputHints,
    pub input: Input,
}
impl Validate for WriteTerminal {
    fn validate(&self) -> Result<(), ValidationError> {
        self.control.validate()?;
        self.hints.validate()?;
        self.input.validate()
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InputKind {
    Bytes,
    Key,
    Paste,
    Mouse,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Disposition {
    Accepted,
    Ignored,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptedInput {
    pub input_id: Id,
    pub kind: InputKind,
    pub accepted_bytes: u32,
    pub disposition: Disposition,
}
impl Validate for AcceptedInput {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(
            self.accepted_bytes as usize <= MAX_ENCODED_INPUT_BYTES
                && (self.disposition == Disposition::Ignored) == (self.accepted_bytes == 0),
        )
    }
}
impl AcceptedInput {
    pub fn check_input(&self, input: &WriteTerminal) -> Result<(), ValidationError> {
        self.validate()?;
        ensure(self.input_id == input.input_id && self.kind == input.input.kind())?;
        if let Input::Bytes { data, .. } = &input.input {
            ensure(
                self.disposition == Disposition::Accepted
                    && self.accepted_bytes as usize == crate::chunks::base64_size(data)?,
            )?;
        }
        Ok(())
    }
}
