use super::*;
use std::rc::Weak;
#[path = "presentation_selection.rs"]
mod selection_state;
pub use selection_state::{
    HistoryPoint, PresentationBuffer, PresentationSelection, PresentationSelectionKind,
};

pub const MAX_PRESENTATION_VIEWS: usize = 17;
/// Default frame policy. Callers may explicitly raise it to the hard engine bound.
pub const MAX_PRESENTATION_CELLS: usize = 262_144;
pub const HARD_PRESENTATION_CELLS: usize = 1_048_576;
#[derive(Debug)]
pub struct PresentationView {
    ptr: NonNull<c_void>,
    owner: Weak<()>,
}
impl Drop for PresentationView {
    fn drop(&mut self) {
        // SAFETY: tracked refs explicitly support freeing after their terminal.
        unsafe { sf_presentation_free(self.ptr.as_ptr()) }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowMetadata {
    pub soft_wrapped: bool,
    pub wrap_continuation: bool,
}
pub struct PresentationFrame {
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<Cell>,
    pub metadata: Vec<RowMetadata>,
    pub cursor: Cursor,
    pub top_row: u64,
    pub total_rows: u64,
}
#[repr(C)]
#[derive(Default)]
struct NativeFrame {
    top_row: u64,
    total_rows: u64,
    cols: u16,
    rows: u16,
    cursor_x: u16,
    cursor_y: u16,
    cursor_visible: u8,
}
#[repr(C)]
#[derive(Default)]
struct NativeCell {
    codepoints: *const u32,
    len: usize,
    fg: [u8; 3],
    bg: [u8; 3],
    flags: u16,
    width: u8,
    selected: u8,
    has_hyperlink: u8,
    soft_wrapped: u8,
    wrap_continuation: u8,
}
#[repr(C)]
#[derive(Default, Debug, Clone, Copy)]
pub struct CursorVisual {
    pub x: u16,
    pub y: u16,
    pub shape: u8,
    pub blinking: u8,
    pub visible: u8,
    pub width: u8,
    pub wide_tail: u8,
    pub color: [u8; 3],
}
unsafe extern "C" {
    fn sf_presentation_new(out: *mut *mut c_void) -> i32;
    fn sf_presentation_free(view: *mut c_void);
    fn sf_presentation_scroll(vt: *mut c_void, view: *mut c_void, delta: isize, follow: u8) -> i32;
    fn sf_presentation_select(vt: *mut c_void, view: *mut c_void, input: *const Selection) -> i32;
    fn sf_presentation_copy(
        vt: *mut c_void,
        view: *mut c_void,
        out: *mut u8,
        cap: usize,
        len: *mut usize,
    ) -> i32;
    fn sf_presentation_hyperlink(
        vt: *mut c_void,
        view: *mut c_void,
        x: u16,
        y: u16,
        out: *mut u8,
        cap: usize,
        len: *mut usize,
    ) -> i32;
    fn sf_presentation_begin(vt: *mut c_void, view: *mut c_void, out: *mut NativeFrame) -> i32;
    fn sf_presentation_cell(vt: *mut c_void, view: *mut c_void, out: *mut NativeCell) -> i32;
    fn sf_cursor_visual(vt: *mut c_void, out: *mut CursorVisual) -> i32;
}
impl Engine {
    fn check_view(&self, view: &PresentationView) -> Result<()> {
        self.status()?;
        if !Weak::ptr_eq(&view.owner, &Rc::downgrade(&self.presentation_owner)) {
            return Err(-2);
        }
        Ok(())
    }
    pub fn new_presentation_view(&mut self) -> Result<PresentationView> {
        self.status()?;
        if Rc::weak_count(&self.presentation_owner) >= MAX_PRESENTATION_VIEWS {
            return Err(-6);
        }
        let mut ptr = std::ptr::null_mut();
        // SAFETY: initialized output receives an independently owned view.
        unsafe {
            check(sf_presentation_new(&mut ptr))?;
        }
        Ok(PresentationView {
            ptr: NonNull::new(ptr).ok_or(-2)?,
            owner: Rc::downgrade(&self.presentation_owner),
        })
    }
    pub fn view_scroll(&mut self, view: &mut PresentationView, delta: isize) -> Result<()> {
        self.check_view(view)?;
        // SAFETY: matching live owner; C changes only view-owned tracked refs.
        unsafe {
            check(sf_presentation_scroll(
                self.ptr.as_ptr(),
                view.ptr.as_ptr(),
                delta,
                0,
            ))
        }
    }
    pub fn view_follow_tail(&mut self, view: &mut PresentationView) -> Result<()> {
        self.check_view(view)?;
        // SAFETY: matching live owner; no source viewport or VT mutation.
        unsafe {
            check(sf_presentation_scroll(
                self.ptr.as_ptr(),
                view.ptr.as_ptr(),
                0,
                1,
            ))
        }
    }
    pub fn view_select(
        &mut self,
        view: &mut PresentationView,
        selection: Option<Selection>,
    ) -> Result<()> {
        self.check_view(view)?;
        if selection.is_some_and(|s| s.kind > 3 || s.rectangle > 1) {
            return Err(-2);
        }
        // SAFETY: C derives selection using fresh refs and tracks both ends before returning.
        unsafe {
            check(sf_presentation_select(
                self.ptr.as_ptr(),
                view.ptr.as_ptr(),
                selection
                    .as_ref()
                    .map_or(std::ptr::null(), std::ptr::from_ref),
            ))
        }
    }
    pub fn view_copy(&mut self, view: &mut PresentationView) -> Result<Option<String>> {
        self.check_view(view)?;
        // SAFETY: both synchronous copies use the same unique owner and selection.
        let bytes = owned_bytes(LIMIT, |out, cap, len| unsafe {
            sf_presentation_copy(self.ptr.as_ptr(), view.ptr.as_ptr(), out, cap, len)
        });
        match bytes {
            Err(-4) => Ok(None),
            result => String::from_utf8(result?).map(Some).map_err(|_| -2),
        }
    }
    pub fn view_hyperlink(
        &mut self,
        view: &mut PresentationView,
        x: u16,
        y: u16,
    ) -> Result<Option<String>> {
        self.check_view(view)?;
        // SAFETY: matching source; fresh view-relative ref is resolved and copied per call.
        let bytes = owned_bytes(LIMIT, |out, cap, len| unsafe {
            sf_presentation_hyperlink(self.ptr.as_ptr(), view.ptr.as_ptr(), x, y, out, cap, len)
        })?;
        let text = String::from_utf8(bytes).map_err(|_| -2)?;
        Ok(if text.is_empty() { None } else { Some(text) })
    }
    pub fn view_render(
        &mut self,
        view: &mut PresentationView,
        max_cells: usize,
    ) -> Result<PresentationFrame> {
        self.check_view(view)?;
        if max_cells == 0 || max_cells > HARD_PRESENTATION_CELLS {
            return Err(-2);
        }
        let mut frame = NativeFrame::default();
        // SAFETY: begins one owner-atomic read. No terminal mutation may interleave its cells.
        unsafe {
            check(sf_presentation_begin(
                self.ptr.as_ptr(),
                view.ptr.as_ptr(),
                &mut frame,
            ))?;
        }
        dimensions(frame.cols, frame.rows)?;
        let count = usize::from(frame.cols) * usize::from(frame.rows);
        if count > max_cells {
            return Err(-6);
        }
        if frame.cols != self.cols
            || frame.rows != self.rows
            || frame.total_rows < u64::from(frame.rows)
            || frame.top_row > frame.total_rows - u64::from(frame.rows)
            || frame.cursor_visible > 1
            || frame.cursor_x >= frame.cols
            || (frame.cursor_visible != 0 && frame.cursor_y >= frame.rows)
        {
            return Err(-2);
        }
        let mut cells = Vec::new();
        cells.try_reserve_exact(count).map_err(|_| -1)?;
        let mut metadata = Vec::new();
        metadata
            .try_reserve_exact(usize::from(frame.rows))
            .map_err(|_| -1)?;
        let mut text_bytes = 0usize;
        for index in 0..count {
            let mut cell = NativeCell::default();
            // SAFETY: fresh public grid refs are created and read only within this C call.
            unsafe {
                check(sf_presentation_cell(
                    self.ptr.as_ptr(),
                    view.ptr.as_ptr(),
                    &mut cell,
                ))?;
            }
            if cell.len > LIMIT / 4
                || cell.width > 2
                || cell.selected > 1
                || cell.has_hyperlink > 1
                || cell.soft_wrapped > 1
                || cell.wrap_continuation > 1
            {
                return Err(-2);
            }
            let codepoints = if cell.len == 0 {
                &[]
            } else {
                if cell.codepoints.is_null() {
                    return Err(-2);
                }
                // SAFETY: C owns this bounded buffer until the next native call, copied immediately.
                unsafe { std::slice::from_raw_parts(cell.codepoints, cell.len) }
            };
            let mut text = String::new();
            text.try_reserve_exact(cell.len * 4).map_err(|_| -1)?;
            for &codepoint in codepoints {
                text.push(char::from_u32(codepoint).ok_or(-2)?);
            }
            text_bytes = text_bytes
                .checked_add(text.len())
                .filter(|n| *n <= 16 * LIMIT)
                .ok_or(-6)?;
            cells.push(Cell {
                text,
                fg: cell.fg,
                bg: cell.bg,
                flags: cell.flags,
                width: cell.width,
                selected: cell.selected != 0,
                has_hyperlink: cell.has_hyperlink != 0,
            });
            if index % usize::from(frame.cols) == 0 {
                metadata.push(RowMetadata {
                    soft_wrapped: cell.soft_wrapped != 0,
                    wrap_continuation: cell.wrap_continuation != 0,
                });
            }
        }
        Ok(PresentationFrame {
            cols: frame.cols,
            rows: frame.rows,
            cells,
            metadata,
            cursor: Cursor {
                x: frame.cursor_x,
                y: frame.cursor_y,
                visible: frame.cursor_visible,
            },
            top_row: frame.top_row,
            total_rows: frame.total_rows,
        })
    }
    pub fn cursor_visual(&mut self) -> Result<CursorVisual> {
        self.status()?;
        let mut cursor = CursorVisual::default();
        // SAFETY: reads public render/terminal metadata into an initialized fixed-layout struct.
        unsafe {
            check(sf_cursor_visual(self.ptr.as_ptr(), &mut cursor))?;
        }
        if cursor.x >= self.cols
            || cursor.y >= self.rows
            || cursor.shape > 3
            || cursor.blinking > 1
            || cursor.visible > 1
            || !(1..=2).contains(&cursor.width)
            || cursor.wide_tail > 1
            || (cursor.wide_tail != 0 && cursor.x == 0)
        {
            return Err(-2);
        }
        Ok(cursor)
    }
}
