//! Owned access to the pinned public libghostty-vt ABI through a C adapter.
//! No raw handles or borrowed library memory escape this crate.
use std::{
    ffi::{c_char, c_void, CStr},
    marker::PhantomData,
    ptr::NonNull,
    rc::Rc,
};

mod colors;
mod input;
pub use input::XtermInputMode;
mod interaction;
mod key_v2;
mod metadata;
mod presentation;
mod recovery;
pub use interaction::{Mouse, Selection};
pub use key_v2::{InvalidKeyV2, KeyActionV2, KeyV2, ModifiersV2, PhysicalKey};
pub use metadata::{
    TerminalMetadata, MAX_METADATA_CWD_BYTES, MAX_METADATA_TITLE_BYTES, MAX_METADATA_TITLE_UNITS,
    MAX_PENDING_BELLS, MAX_RAW_TITLE_BYTES,
};
pub use presentation::{
    CursorVisual, HistoryPoint, PresentationBuffer, PresentationFrame, PresentationSelection,
    PresentationSelectionKind, PresentationView, RowMetadata, HARD_PRESENTATION_CELLS,
    MAX_PRESENTATION_CELLS, MAX_PRESENTATION_VIEWS,
};
pub use recovery::RecoveryInfo;

pub const MAX_FORMAT_BYTES: usize = 16 * 1024 * 1024;
pub const CONTINUATION_MAX_BYTES: usize = 64 * 1024;

pub const LIMIT: usize = 1024 * 1024;
pub type Result<T> = std::result::Result<T, i32>;
#[repr(C)]
#[derive(Default, Debug, Clone, Copy)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub visible: u8,
}
#[repr(C)]
#[derive(Default)]
struct CellView {
    text: *const u8,
    len: usize,
    fg: [u8; 3],
    bg: [u8; 3],
    flags: u16,
    width: u8,
    selected: u8,
    has_hyperlink: u8,
}
#[derive(Debug, Clone)]
pub struct Cell {
    pub text: String,
    pub fg: [u8; 3],
    pub bg: [u8; 3],
    pub flags: u16,
    pub width: u8,
    pub selected: bool,
    pub has_hyperlink: bool,
}
pub struct Key<'a> {
    pub key: u32,
    pub action: u32,
    pub mods: u32,
    pub consumed_mods: u32,
    pub unshifted: u32,
    pub composing: bool,
    pub text: &'a str,
}

unsafe extern "C" {
    fn sf_status(vt: *mut c_void) -> i32;
    fn sf_new(cols: u16, rows: u16, out: *mut *mut c_void) -> i32;
    fn sf_free(vt: *mut c_void);
    fn sf_feed(vt: *mut c_void, data: *const u8, len: usize) -> i32;
    fn sf_resize(vt: *mut c_void, cols: u16, rows: u16) -> i32;
    fn sf_replies(vt: *mut c_void, out: *mut u8, len: *mut usize) -> i32;
    fn sf_render(vt: *mut c_void, cursor: *mut Cursor) -> i32;
    fn sf_cell(vt: *mut c_void, cell: *mut CellView) -> i32;
    fn sf_scroll(vt: *mut c_void, delta: isize) -> i32;
    fn sf_version() -> *const c_char;
}
fn check(code: i32) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(code)
    }
}
fn buffer(len: usize) -> Result<Vec<u8>> {
    bounded_buffer(len, LIMIT)
}
fn bounded_buffer(len: usize, max_bytes: usize) -> Result<Vec<u8>> {
    if len > max_bytes {
        return Err(-6);
    }
    let mut value = Vec::new();
    value.try_reserve_exact(len).map_err(|_| -1)?;
    value.resize(len, 0);
    Ok(value)
}
/// Two-pass copy with no truncation. Callers serialize access and must not
/// mutate native state or retain borrowed references between calls.
fn owned_bytes(
    max_bytes: usize,
    mut call: impl FnMut(*mut u8, usize, &mut usize) -> i32,
) -> Result<Vec<u8>> {
    let mut len = 0;
    let r = call(std::ptr::null_mut(), 0, &mut len);
    if r != 0 && r != -3 {
        return Err(r);
    }
    let mut out = bounded_buffer(len, max_bytes)?;
    check(call(out.as_mut_ptr(), out.len(), &mut len))?;
    if len > out.len() {
        return Err(-2);
    }
    out.truncate(len);
    Ok(out)
}
fn dimensions(cols: u16, rows: u16) -> Result<()> {
    if cols == 0 || rows == 0 || usize::from(cols) * usize::from(rows) > LIMIT {
        Err(-2)
    } else {
        Ok(())
    }
}

/// Thread-confined owner. The C adapter owns callbacks, buffers and iterators.
pub struct Engine {
    ptr: NonNull<c_void>,
    cols: u16,
    rows: u16,
    _thread: PhantomData<Rc<()>>,
    presentation_owner: Rc<()>,
}
impl Engine {
    pub fn new(cols: u16, rows: u16) -> Result<Self> {
        dimensions(cols, rows)?;
        let mut ptr = std::ptr::null_mut();
        // SAFETY: output points to live storage. Adapter returns an owned handle on success.
        unsafe {
            check(sf_new(cols, rows, &mut ptr))?;
        }
        let engine = Self {
            ptr: NonNull::new(ptr).ok_or(-2)?,
            cols,
            rows,
            _thread: PhantomData,
            presentation_owner: Rc::new(()),
        };
        // SAFETY: the public version function returns a static NUL-terminated string.
        let version = unsafe { CStr::from_ptr(sf_version()) };
        if version.to_bytes() != b"1.0.1" {
            return Err(-8);
        }
        Ok(engine)
    }
    /// Sticky callback failure takes precedence over argument validation.
    pub fn status(&self) -> Result<()> {
        // SAFETY: the thread-confined owner keeps the adapter alive.
        unsafe { check(sf_status(self.ptr.as_ptr())) }
    }
    pub fn feed(&mut self, data: &[u8]) -> Result<()> {
        self.status()?;
        if data.len() > LIMIT {
            return Err(-6);
        }
        // SAFETY: the unique owner prevents concurrent access; data is borrowed for this call.
        unsafe { check(sf_feed(self.ptr.as_ptr(), data.as_ptr(), data.len())) }
    }
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.status()?;
        dimensions(cols, rows)?;
        // SAFETY: valid owned handle and validated nonzero dimensions.
        unsafe {
            check(sf_resize(self.ptr.as_ptr(), cols, rows))?;
        }
        self.cols = cols;
        self.rows = rows;
        Ok(())
    }
    pub fn drain_replies(&mut self) -> Result<Vec<u8>> {
        let mut len = 0;
        // SAFETY: NULL queries length without modifying storage. No other call can intervene.
        unsafe {
            check(sf_replies(
                self.ptr.as_ptr(),
                std::ptr::null_mut(),
                &mut len,
            ))?;
        }
        let mut out = buffer(len)?;
        // SAFETY: output has exactly the queried size; callback state cannot change concurrently.
        unsafe {
            check(sf_replies(self.ptr.as_ptr(), out.as_mut_ptr(), &mut len))?;
        }
        if out.len() != len {
            return Err(-2);
        }
        Ok(out)
    }
    pub fn render(&mut self) -> Result<(Vec<Cell>, Cursor)> {
        let mut cursor = Cursor::default();
        // SAFETY: all output storage is initialized; the adapter exclusively updates owned state.
        unsafe {
            check(sf_render(self.ptr.as_ptr(), &mut cursor))?;
        }
        if cursor.visible != 0 && (cursor.x >= self.cols || cursor.y >= self.rows) {
            return Err(-2);
        }
        let count = usize::from(self.cols) * usize::from(self.rows);
        let mut cells = Vec::new();
        cells.try_reserve_exact(count).map_err(|_| -1)?;
        let mut text_bytes = 0usize;
        for _ in 0..count {
            let mut view = CellView::default();
            // SAFETY: output is initialized. view.text remains valid until the next adapter call.
            unsafe {
                check(sf_cell(self.ptr.as_ptr(), &mut view))?;
            }
            text_bytes = text_bytes.checked_add(view.len).ok_or(-6)?;
            if view.len > LIMIT || text_bytes > 16 * LIMIT || view.width > 2 {
                return Err(-6);
            }
            if view.selected > 1 || view.has_hyperlink > 1 {
                return Err(-6);
            }
            let bytes = if view.len == 0 {
                &[]
            } else {
                if view.text.is_null() {
                    return Err(-2);
                }
                // SAFETY: adapter returns its owned text allocation with checked length and lifetime.
                unsafe { std::slice::from_raw_parts(view.text, view.len) }
            };
            let text = std::str::from_utf8(bytes).map_err(|_| -2)?;
            let mut owned = String::new();
            owned.try_reserve_exact(text.len()).map_err(|_| -1)?;
            owned.push_str(text);
            cells.push(Cell {
                text: owned,
                fg: view.fg,
                bg: view.bg,
                flags: view.flags,
                width: view.width,
                selected: view.selected != 0,
                has_hyperlink: view.has_hyperlink != 0,
            });
        }
        Ok((cells, cursor))
    }
    pub fn scroll(&mut self, delta: isize) -> Result<()> {
        // SAFETY: unique live owner; scroll is a synchronous public ABI operation.
        unsafe { check(sf_scroll(self.ptr.as_ptr(), delta)) }
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: exactly one owner; frees callbacks and every allocation before invalidating handle.
        unsafe { sf_free(self.ptr.as_ptr()) }
    }
}
