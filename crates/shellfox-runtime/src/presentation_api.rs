use crate::{Identity, Lease, ObserverLease, ScreenFrame};
pub use shellfox_ghostty_vt::{
    CursorShape, CursorVisual, HistoryPoint, PresentationBuffer, PresentationSelection,
    PresentationSelectionKind, RowMetadata,
};
use shellfox_ghostty_vt::{Selection, ViewportPoint};
use std::sync::Arc;

/// Read-only presentation authority is separate from terminal control authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum PresentationLease {
    Controller(Lease),
    Observer(ObserverLease),
}
#[derive(Debug)]
pub enum PresentationAction {
    Frame {
        focused: bool,
    },
    Scroll {
        delta: isize,
    },
    FollowTail,
    /// Resizes the canonical external grid shared by all views, not per-view reflow.
    /// Callers must separately authorize this layout change; delivery alone is not enough.
    /// This never resizes or otherwise controls an upstream raw process.
    ResizeExternal {
        cols: u16,
        rows: u16,
    },
    Selection {
        selection: Option<Selection>,
    },
    /// AT coordinates belong to this exact sampled view version. Unlike an
    /// ordinary gesture, stale coordinates must be rejected, never retargeted.
    SelectionAtVersion {
        version: u64,
        selection: Option<Selection>,
    },
    Copy,
    Hyperlink {
        point: ViewportPoint,
    },
}
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewFrame {
    pub identity: Identity,
    pub view: PresentationLease,
    /// Highest source output parsed into this immutable frame.
    pub sequence: u64,
    /// View-local repaint identity. Selection/focus can change at the same sequence.
    pub version: u64,
    pub screen: Arc<ScreenFrame>,
    pub row_metadata: Arc<[RowMetadata]>,
    pub cursor_visual: CursorVisual,
    /// Actual tracked, cell-inclusive endpoints in retained buffer history.
    /// Columns address published VT cells, not re-segmented Unicode or UTF-16.
    /// Range retains anchor/focus direction; Word/Line/All are normalized forward.
    /// None also covers a pruned endpoint, never a guessed/clamped visible range.
    pub selection: Option<PresentationSelection>,
    pub top_row: u64,
    pub total_rows: u64,
}
