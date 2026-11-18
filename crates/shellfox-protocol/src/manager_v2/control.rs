use super::*;
use crate::{validation::*, Validate, ValidationError};
use serde::{Deserialize, Serialize};
pub const MAX_GRID_AXIS: u16 = 32767;
pub const MAX_GRID_CELLS: usize = 1048576;
pub fn validate_grid(cols: u16, rows: u16) -> Result<(), ValidationError> {
    ensure(
        cols > 0
            && rows > 0
            && cols <= MAX_GRID_AXIS
            && rows <= MAX_GRID_AXIS
            && usize::from(cols) * usize::from(rows) <= MAX_GRID_CELLS,
    )
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ControlRef {
    pub tab_id: Id,
    pub generation: Id,
    pub control_id: Id,
    pub control_epoch: u64,
}
impl Validate for ControlRef {
    fn validate(&self) -> Result<(), ValidationError> {
        epoch(self.control_epoch)
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayoutRef {
    pub tab_id: Id,
    pub generation: Id,
    pub layout_id: Id,
    pub layout_epoch: u64,
}
impl Validate for LayoutRef {
    fn validate(&self) -> Result<(), ValidationError> {
        epoch(self.layout_epoch)
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ControlRequest {
    None,
    IfFree,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttachTerminal {
    pub tab_id: Id,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub resume: Option<Resume>,
    pub control: ControlRequest,
}
impl Validate for AttachTerminal {
    fn validate(&self) -> Result<(), ValidationError> {
        if let Some(v) = &self.resume {
            v.validate()?;
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InputState {
    Owned,
    Busy,
    NotReady,
    IntrinsicReadOnly,
    SourcePolicyDenied,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Attached {
    pub delivery: DeliveryRef,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub input: Option<ControlRef>,
    pub input_state: InputState,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub layout: Option<LayoutRef>,
    pub recovery: Recovery,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Attachment {
    Attached(Box<Attached>),
    #[serde(rename_all = "camelCase")]
    Unavailable {
        tab_id: Id,
        reason: UnavailableReason,
    },
}
impl Validate for Attachment {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Attached(value) => {
                let Attached {
                    delivery,
                    input,
                    input_state,
                    layout,
                    recovery,
                } = &**value;
                delivery.validate()?;
                recovery.validate()?;
                ensure((*input_state == InputState::Owned) == input.is_some())?;
                if let Some(input) = input {
                    input.validate()?;
                    ensure(
                        input.tab_id == delivery.tab_id
                            && input.generation == delivery.generation
                            && input.control_epoch == delivery.delivery_epoch,
                    )?;
                }
                if let Some(layout) = layout {
                    layout.validate()?;
                    ensure(
                        input.is_some()
                            && layout.tab_id == delivery.tab_id
                            && layout.generation == delivery.generation,
                    )?;
                }
                Ok(())
            }
            Self::Unavailable { .. } => Ok(()),
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum TakeMode {
    #[serde(rename = "takeover")]
    Takeover,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TakeTerminalControl {
    pub delivery: DeliveryRef,
    pub mode: TakeMode,
}
impl Validate for TakeTerminalControl {
    fn validate(&self) -> Result<(), ValidationError> {
        self.delivery.validate()
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResizeTerminal {
    pub control: ControlRef,
    pub layout: LayoutRef,
    pub expected_layout_revision: u64,
    pub cols: u16,
    pub rows: u16,
}
impl Validate for ResizeTerminal {
    fn validate(&self) -> Result<(), ValidationError> {
        self.control.validate()?;
        self.layout.validate()?;
        epoch(self.expected_layout_revision)?;
        ensure(
            self.control.tab_id == self.layout.tab_id
                && self.control.generation == self.layout.generation,
        )?;
        validate_grid(self.cols, self.rows)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Resized {
    pub cols: u16,
    pub rows: u16,
    pub layout_revision: u64,
}
impl Validate for Resized {
    fn validate(&self) -> Result<(), ValidationError> {
        validate_grid(self.cols, self.rows)?;
        epoch(self.layout_revision)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DetachTerminal {
    pub delivery: DeliveryRef,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub release_control: Option<ControlRef>,
}
impl Validate for DetachTerminal {
    fn validate(&self) -> Result<(), ValidationError> {
        self.delivery.validate()?;
        if let Some(v) = &self.release_control {
            v.validate()?;
            ensure(
                v.tab_id == self.delivery.tab_id
                    && v.generation == self.delivery.generation
                    && v.control_epoch == self.delivery.delivery_epoch,
            )?;
        }
        Ok(())
    }
}
