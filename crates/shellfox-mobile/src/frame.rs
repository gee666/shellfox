use crate::engine::{Owned, Result, Status};
use std::ffi::c_void;

#[repr(C)]
pub struct SfmCell {
    pub text: *const u8,
    pub text_length: usize,
    pub foreground: u32,
    pub background: u32,
    pub flags: u16,
    pub width: u8,
}
#[repr(C)]
pub struct SfmFrame {
    pub cols: u16,
    pub rows: u16,
    pub cursor_x: u16,
    pub cursor_y: u16,
    pub cursor_visible: u8,
    pub cells: *const SfmCell,
    pub cell_count: usize,
    pub consumed_sequence: u64,
}
pub type FrameFn = extern "C" fn(*mut c_void, *const SfmFrame);
fn rgb(c: shellfox_ghostty_vt::Rgb) -> u32 {
    u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)
}
pub(crate) fn frame(owned: &mut Owned, callback: FrameFn, context: *mut c_void) -> Result<()> {
    let screen = owned.terminal.render().map_err(|_| Status::Engine)?;
    let cells: Vec<_> = screen
        .cells
        .iter()
        .map(|c| SfmCell {
            text: c.text.as_ptr(),
            text_length: c.text.len(),
            foreground: rgb(c.foreground),
            background: rgb(c.background),
            flags: u16::from(c.bold)
                | u16::from(c.italic) << 1
                | u16::from(c.underline) << 2
                | u16::from(c.inverse) << 3
                | u16::from(c.faint) << 4
                | u16::from(c.invisible) << 5
                | u16::from(c.strikethrough) << 6
                | u16::from(c.selected) << 7
                | u16::from(c.has_hyperlink) << 8,
            width: c.width,
        })
        .collect();
    let consumed_sequence = owned
        .shared
        .queue
        .lock()
        .map_err(|_| Status::Panic)?
        .consumed;
    let frame = SfmFrame {
        cols: screen.cols,
        rows: screen.rows,
        cursor_x: screen.cursor.x,
        cursor_y: screen.cursor.y,
        cursor_visible: u8::from(screen.cursor.visible),
        cells: cells.as_ptr(),
        cell_count: cells.len(),
        consumed_sequence,
    };
    callback(context, &frame);
    Ok(())
}
