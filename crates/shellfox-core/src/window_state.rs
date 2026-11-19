//! The five Electron window-state fields. Chrome changes do not affect session revisions.
use serde::{Deserialize, Serialize};

pub const DEFAULT_WIDTH: i32 = 1220;
pub const DEFAULT_HEIGHT: i32 = 820;
pub const MIN_WIDTH: i32 = 850;
pub const MIN_HEIGHT: i32 = 600;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
}

impl WindowState {
    pub fn valid(self) -> bool {
        (-100_000..=100_000).contains(&self.x)
            && (-100_000..=100_000).contains(&self.y)
            && (MIN_WIDTH..=20_000).contains(&self.width)
            && (MIN_HEIGHT..=20_000).contains(&self.height)
    }

    pub fn parse(value: &[u8]) -> Option<Self> {
        let state: Self = serde_json::from_slice(value).ok()?;
        state.valid().then_some(state)
    }
}
