use crate::{Error, Result, Terminal};
use shellfox_ghostty_vt_sys as sys;

/// Zero-based cell in the currently visible viewport, not the active screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewportPoint {
    pub x: u16,
    pub y: u16,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Selection {
    /// Both endpoints are inclusive. Reversed and rectangular ranges are valid.
    Range {
        start: ViewportPoint,
        end: ViewportPoint,
        rectangle: bool,
    },
    Word(ViewportPoint),
    Line(ViewportPoint),
    /// Includes scrollback, using Ghostty's select-all semantics.
    All,
}
pub(crate) fn native_selection(selection: Selection) -> sys::Selection {
    let (kind, start, end, rectangle) = match selection {
        Selection::Range {
            start,
            end,
            rectangle,
        } => (0, start, end, rectangle),
        Selection::Word(point) => (1, point, point, false),
        Selection::Line(point) => (2, point, point, false),
        Selection::All => (
            3,
            ViewportPoint { x: 0, y: 0 },
            ViewportPoint { x: 0, y: 0 },
            false,
        ),
    };
    sys::Selection {
        kind,
        start_x: start.x,
        start_y: start.y,
        end_x: end.x,
        end_y: end.y,
        rectangle: u8::from(rectangle),
    }
}
impl Terminal {
    /// Installs a terminal-owned tracked selection, or clears it with None.
    /// Word/line operations with no selectable content clear the old selection.
    /// No untracked grid reference or borrowed library memory leaves this call.
    pub fn set_selection(&mut self, selection: Option<Selection>) -> Result<()> {
        let native = selection.map(native_selection);
        self.engine.set_selection(native).map_err(Error)
    }
    /// Copies the active selection using Ghostty's plain-text formatter, with
    /// trailing whitespace trimmed and soft-wrapped lines joined. Returns None
    /// without an active selection.
    /// Output is owned UTF-8, capped at 1 MiB. This never touches the OS clipboard.
    pub fn copy_selection(&mut self) -> Result<Option<String>> {
        self.engine.copy_selection().map_err(Error)
    }
    /// Returns an owned OSC 8 URI, capped at 1 MiB. It is untrusted metadata,
    /// not permission to launch anything. The host/UI must validate schemes,
    /// control characters, paths and user intent before opening it.
    pub fn hyperlink_at(&mut self, point: ViewportPoint) -> Result<Option<String>> {
        self.engine.hyperlink_at(point.x, point.y).map_err(Error)
    }
}
