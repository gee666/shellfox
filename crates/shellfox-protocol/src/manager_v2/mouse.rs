use super::*;
use crate::{validation::*, Validate, ValidationError};
use serde::{Deserialize, Serialize};
/// Existing mouse encoder state. Keyboard input uses the independent full ModifiersV2 mask.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    #[serde(rename = "super")]
    pub super_key: bool,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MouseAction {
    Press,
    Release,
    Motion,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    WheelLeft,
    WheelRight,
    Back,
    Forward,
    Ten,
    Eleven,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MouseEvent {
    pub action: MouseAction,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub button: Option<MouseButton>,
    pub modifiers: Modifiers,
    pub x_px: f64,
    pub y_px: f64,
    pub any_button_pressed: bool,
}
impl Validate for MouseEvent {
    fn validate(&self) -> Result<(), ValidationError> {
        ensure(
            self.x_px.is_finite()
                && self.y_px.is_finite()
                && self.x_px.abs() <= 1e9
                && self.y_px.abs() <= 1e9,
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeometryStamp {
    pub recovery_id: Id,
    pub layout_revision: u64,
    pub cols: u16,
    pub rows: u16,
    pub cell_width_px: u32,
    pub cell_height_px: u32,
}
impl Validate for GeometryStamp {
    fn validate(&self) -> Result<(), ValidationError> {
        validate_grid(self.cols, self.rows)?;
        epoch(self.layout_revision)?;
        ensure(
            self.cell_width_px > 0
                && self.cell_height_px > 0
                && u64::from(self.cols) * u64::from(self.cell_width_px) <= 1000000000
                && u64::from(self.rows) * u64::from(self.cell_height_px) <= 1000000000,
        )
    }
}
