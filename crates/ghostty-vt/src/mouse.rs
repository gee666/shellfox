use crate::{Error, Modifiers, Result, Terminal};
use shellfox_ghostty_vt_sys as sys;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    Motion,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Public button identities with no wire encoding in vt-1.0.1.
    Ten,
    Eleven,
}
impl MouseButton {
    fn code(self) -> u32 {
        // Public GhosttyMouseButton identities. Buttons four through seven
        // are the wheel directions in the public encoder's terminal protocols.
        match self {
            Self::Left => 1,
            Self::Right => 2,
            Self::Middle => 3,
            Self::WheelUp => 4,
            Self::WheelDown => 5,
            Self::WheelLeft => 6,
            Self::WheelRight => 7,
            Self::Back => 8,
            Self::Forward => 9,
            Self::Ten => 10,
            Self::Eleven => 11,
        }
    }
}
/// Content-local pixel coordinates. Negative/outside positions are allowed for
/// drag/release reporting. Positions must be finite and within +/- 1 billion.
/// Wheel steps use Press with a wheel button. Accumulate fractional scrolling
/// in the UI and send each whole step, rather than inventing escape sequences.
#[derive(Debug, Clone, Copy)]
pub struct MouseEvent {
    pub action: MouseAction,
    pub button: Option<MouseButton>,
    pub modifiers: Modifiers,
    pub x: f32,
    pub y: f32,
    /// Current state after this event, including any other held button.
    pub any_button_pressed: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseGeometry {
    pub cell_width: u32,
    pub cell_height: u32,
}
impl Terminal {
    /// Does the application request mouse reporting? A host may still override
    /// it for selection gestures, for example while holding Shift.
    pub fn mouse_tracking(&mut self) -> Result<bool> {
        self.engine.mouse_tracking().map_err(Error)
    }
    /// Ghostty selects tracking and output format from current terminal modes.
    /// Geometry has no padding; subtract the content origin in the UI first.
    /// Screen pixel dimensions must be nonzero and at most 1 billion per axis.
    /// An empty result means Ghostty chose not to report the event.
    pub fn encode_mouse(&mut self, event: &MouseEvent, geometry: MouseGeometry) -> Result<Vec<u8>> {
        self.engine
            .encode_mouse(sys::Mouse {
                action: match event.action {
                    MouseAction::Press => 0,
                    MouseAction::Release => 1,
                    MouseAction::Motion => 2,
                },
                button: event.button.map_or(0, MouseButton::code),
                mods: event.modifiers.bits(),
                cell_width: geometry.cell_width,
                cell_height: geometry.cell_height,
                x: event.x,
                y: event.y,
                any_button_pressed: u8::from(event.any_button_pressed),
            })
            .map_err(Error)
    }
}
