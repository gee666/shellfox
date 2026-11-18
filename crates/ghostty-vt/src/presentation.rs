//! Independent, thread-confined presentation handles over one authoritative VT.
use crate::{Cell, Error, Result, Rgb, Screen, Selection, Terminal};
use shellfox_ghostty_vt_sys as sys;

pub use sys::{
    HistoryPoint, PresentationBuffer, PresentationSelection, PresentationSelectionKind,
    RowMetadata, HARD_PRESENTATION_CELLS, MAX_PRESENTATION_CELLS, MAX_PRESENTATION_VIEWS,
};

/// Owns tracked viewport/selection anchors for this terminal's two buffers.
/// Moving a view to another terminal is rejected. Dropping its source terminal
/// first is safe; the remaining handle can only be dropped, not reused.
/// ```compile_fail
/// fn require_send<T: Send>() {}
/// require_send::<shellfox_ghostty_vt::PresentationView>();
/// ```
/// ```compile_fail
/// fn require_sync<T: Sync>() {}
/// require_sync::<shellfox_ghostty_vt::PresentationView>();
/// ```
#[derive(Debug)]
pub struct PresentationView {
    state: sys::PresentationView,
}

pub struct PresentationFrame {
    pub screen: Screen,
    /// Soft-wrap metadata from public row getters, one entry per visible row.
    pub row_metadata: Vec<RowMetadata>,
    /// Absolute row in the active buffer, including history.
    pub top_row: u64,
    pub total_rows: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Bar,
    Underline,
    HollowBlock,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorVisual {
    /// Active-grid coordinates. Use PresentationFrame.screen.cursor for the
    /// position and visibility relative to a particular view's scroll position.
    pub x: u16,
    pub y: u16,
    pub visible: bool,
    pub shape: CursorShape,
    pub blinking: bool,
    pub color: Rgb,
    pub width: u8,
    /// Cursor x points at a wide-cell tail. Paint from x - 1 in that case.
    pub wide_tail: bool,
}
impl CursorVisual {
    /// Focus is view-local. An inactive outline never changes terminal modes.
    pub fn for_focus(mut self, focused: bool) -> Self {
        if !focused {
            self.shape = CursorShape::HollowBlock;
            self.blinking = false;
        }
        self
    }
}
impl Cell {
    pub(crate) fn from_native(cell: sys::Cell) -> Self {
        Self {
            text: cell.text,
            foreground: cell.fg.into(),
            background: cell.bg.into(),
            bold: cell.flags & 1 != 0,
            italic: cell.flags & 2 != 0,
            underline: cell.flags & 0x700 != 0,
            inverse: cell.flags & 16 != 0,
            faint: cell.flags & 4 != 0,
            invisible: cell.flags & 32 != 0,
            strikethrough: cell.flags & 64 != 0,
            width: cell.width,
            selected: cell.selected,
            has_hyperlink: cell.has_hyperlink,
        }
    }
}
impl Terminal {
    /// At most 17 live handles per terminal: one controller and 16 observers.
    /// This confers no PTY or control authority. Reads need no controller lease.
    pub fn new_presentation_view(&mut self) -> Result<PresentationView> {
        self.engine
            .new_presentation_view()
            .map(|state| PresentationView { state })
            .map_err(Error)
    }
    /// Negative is up. Anchors follow output scrolling and resize/reflow until
    /// pruned; a pruned viewport clamps to the oldest remaining row.
    pub fn view_scroll(&mut self, view: &mut PresentationView, delta: isize) -> Result<()> {
        self.engine
            .view_scroll(&mut view.state, delta)
            .map_err(Error)
    }
    pub fn view_follow_tail(&mut self, view: &mut PresentationView) -> Result<()> {
        self.engine.view_follow_tail(&mut view.state).map_err(Error)
    }
    /// Coordinates are relative to THIS view, not the source's global viewport.
    /// Word and logical-line boundaries come from Ghostty, including Unicode.
    pub fn view_select(
        &mut self,
        view: &mut PresentationView,
        selection: Option<Selection>,
    ) -> Result<()> {
        self.engine
            .view_select(
                &mut view.state,
                selection.map(crate::selection::native_selection),
            )
            .map_err(Error)
    }
    /// Returns actual tracked cell-inclusive endpoints in the retained active
    /// buffer's history coordinates, not clipped or invented viewport positions.
    /// Row zero is the oldest retained row, not a durable output sequence.
    /// Range preserves the caller's original anchor/focus, including reversed
    /// and mirrored rectangles. Word/Line/All use normalized forward endpoints.
    /// The buffer tag identifies primary versus alternate; each has independent
    /// stored selection. Inactive-buffer selections are reported when that buffer
    /// becomes active again. A discarded endpoint returns None, never a guessed
    /// surviving range. No controller selection, source viewport or refs change.
    /// Both endpoints are inclusive cells, not exclusive character boundaries.
    /// See this crate's PRESENTATION_SELECTION.md for directional AccessKit
    /// conversion, wide graphemes, offscreen and rectangle limitations.
    pub fn view_selection(
        &mut self,
        view: &mut PresentationView,
    ) -> Result<Option<PresentationSelection>> {
        self.engine.view_selection(&mut view.state).map_err(Error)
    }
    /// Uses Ghostty's plain formatter with soft wraps joined and trailing
    /// whitespace trimmed. Does not install a host selection or touch a clipboard.
    /// Refuses output above 1 MiB rather than returning partial text.
    pub fn view_copy(&mut self, view: &mut PresentationView) -> Result<Option<String>> {
        self.engine.view_copy(&mut view.state).map_err(Error)
    }
    /// Resolves a link relative to this view. The untrusted URI is not permission
    /// to launch anything; the application must validate scheme and user intent.
    pub fn view_hyperlink(
        &mut self,
        view: &mut PresentationView,
        point: crate::ViewportPoint,
    ) -> Result<Option<String>> {
        self.engine
            .view_hyperlink(&mut view.state, point.x, point.y)
            .map_err(Error)
    }
    /// Public grid reads never change the authoritative viewport or install the
    /// view's selection. The shared render cache is refreshed for resolved
    /// colors; no separate cache can consume and lose a controller update.
    /// Frames default to 262144 cells and a hard 16 MiB of UTF-8 cell text.
    pub fn view_render(&mut self, view: &mut PresentationView) -> Result<PresentationFrame> {
        self.view_render_with_limit(view, MAX_PRESENTATION_CELLS)
    }
    /// Explicit per-call cell budget, from 1 through HARD_PRESENTATION_CELLS.
    /// A smaller budget can protect slow clients without imposing a hidden 4K limit.
    pub fn view_render_with_limit(
        &mut self,
        view: &mut PresentationView,
        max_cells: usize,
    ) -> Result<PresentationFrame> {
        let frame = self
            .engine
            .view_render(&mut view.state, max_cells)
            .map_err(Error)?;
        let mut cells = Vec::new();
        cells
            .try_reserve_exact(frame.cells.len())
            .map_err(|_| Error(-1))?;
        cells.extend(frame.cells.into_iter().map(Cell::from_native));
        Ok(PresentationFrame {
            screen: Screen {
                cols: frame.cols,
                rows: frame.rows,
                cells,
                cursor: crate::Cursor {
                    x: frame.cursor.x,
                    y: frame.cursor.y,
                    visible: frame.cursor.visible != 0,
                },
            },
            row_metadata: frame.metadata,
            top_row: frame.top_row,
            total_rows: frame.total_rows,
        })
    }
    /// Public DECSCUSR/blink/color and wide-cell information. The shared render
    /// cache is refreshed so a later controller render cannot lose dirty output.
    pub fn cursor_visual(&mut self) -> Result<CursorVisual> {
        let cursor = self.engine.cursor_visual().map_err(Error)?;
        let shape = match cursor.shape {
            0 => CursorShape::Bar,
            1 => CursorShape::Block,
            2 => CursorShape::Underline,
            3 => CursorShape::HollowBlock,
            _ => return Err(Error(-2)),
        };
        Ok(CursorVisual {
            x: cursor.x,
            y: cursor.y,
            visible: cursor.visible != 0,
            shape,
            blinking: cursor.blinking != 0,
            color: cursor.color.into(),
            width: cursor.width,
            wide_tail: cursor.wide_tail != 0,
        })
    }
}
