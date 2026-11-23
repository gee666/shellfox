//! Desktop window bounds in GPUI coordinates. Chrome changes do not affect session revisions.
use serde::{Deserialize, Serialize};

/// Electron's outer window sizes. GPUI sizes are client sizes; see [`client_size`].
pub const DEFAULT_WIDTH: i32 = 1220;
pub const DEFAULT_HEIGHT: i32 = 820;
pub const MIN_WIDTH: i32 = 850;
pub const MIN_HEIGHT: i32 = 600;

/// Normal (not maximized) client bounds in logical screen pixels, relative to
/// the origin of the display they were on. Display-relative bounds survive DPI
/// changes and a rearranged monitor layout. Screen coordinates ignore where the
/// taskbar is, as Electron's did; the desktop converts to window coordinates.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
    /// GPUI display UUID. None means the primary display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

impl WindowState {
    pub fn valid(&self) -> bool {
        (-100_000..=100_000).contains(&self.x)
            && (-100_000..=100_000).contains(&self.y)
            && (100..=20_000).contains(&self.width)
            && (100..=20_000).contains(&self.height)
            && self.display.as_ref().is_none_or(|id| {
                !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_graphic())
            })
    }

    pub fn parse(value: &[u8]) -> Option<Self> {
        let state: Self = serde_json::from_slice(value).ok()?;
        state.valid().then_some(state)
    }

    /// Converts Electron's record once, at import. Electron stored outer bounds
    /// in screen DIPs built outward from the primary display. Those equal GPUI's
    /// primary-relative coordinates on the primary display and on monitors with
    /// the same scale. Windows on a mixed-DPI secondary are clamped on-screen at restore.
    pub fn parse_electron(value: &[u8]) -> Option<Self> {
        #[derive(Deserialize)]
        struct Electron {
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            maximized: bool,
        }
        let old: Electron = serde_json::from_slice(value).ok()?;
        let valid = (-100_000..=100_000).contains(&old.x)
            && (-100_000..=100_000).contains(&old.y)
            && (MIN_WIDTH..=20_000).contains(&old.width)
            && (MIN_HEIGHT..=20_000).contains(&old.height);
        if !valid {
            return None;
        }
        let (width, height) = client_size(old.width, old.height);
        let (left, top) = FRAME_OFFSET;
        let state = Self {
            x: old.x + left,
            y: old.y + top,
            width,
            height,
            maximized: old.maximized,
            display: None,
        };
        state.valid().then_some(state)
    }
}

// Windows' standard frame at 96 DPI, in logical pixels: 8 px sides and bottom,
// 31 px top. GPUI splits the frame evenly around the client area.
#[cfg(windows)]
const FRAME: (i32, i32) = (16, 39);
#[cfg(not(windows))]
const FRAME: (i32, i32) = (0, 0);
const FRAME_OFFSET: (i32, i32) = (FRAME.0 / 2, FRAME.1 / 2);

/// Client size of a window with Electron's outer size. Identity outside Windows.
pub fn client_size(width: i32, height: i32) -> (i32, i32) {
    (width - FRAME.0, height - FRAME.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn electron_outer_bounds_become_primary_relative_client_bounds() {
        let state = WindowState::parse_electron(
            br#"{"x":100,"y":50,"width":1220,"height":820,"maximized":true}"#,
        )
        .unwrap();
        let expected = if cfg!(windows) {
            (108, 69, 1204, 781)
        } else {
            (100, 50, 1220, 820)
        };
        assert_eq!((state.x, state.y, state.width, state.height), expected);
        assert!(state.maximized);
        assert_eq!(state.display, None);
    }

    #[test]
    fn rejects_electron_records_outside_its_schema() {
        for value in [
            &br#"{"x":0,"y":0,"width":849,"height":700,"maximized":false}"#[..],
            br#"{"x":0,"y":0,"width":900,"height":700}"#,
            br#"{"x":200000,"y":0,"width":900,"height":700,"maximized":false}"#,
            b"null",
        ] {
            assert_eq!(WindowState::parse_electron(value), None);
        }
    }

    #[test]
    fn round_trips_display_relative_bounds() {
        let state = WindowState {
            x: -20,
            y: 30,
            width: 900,
            height: 640,
            maximized: false,
            display: Some("5bd2c4e3-0d43-55c4-9b58-1d4a8e3f6a3c".into()),
        };
        let bytes = serde_json::to_vec(&state).unwrap();
        assert_eq!(WindowState::parse(&bytes), Some(state));
        assert_eq!(
            WindowState::parse(br#"{"x":0,"y":0,"width":900,"height":640,"maximized":false}"#)
                .unwrap()
                .display,
            None
        );
        assert_eq!(
            WindowState::parse(
                br#"{"x":0,"y":0,"width":900,"height":640,"maximized":false,"display":"a b"}"#
            ),
            None
        );
    }
}
