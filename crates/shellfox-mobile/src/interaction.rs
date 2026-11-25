use crate::{
    abi::{boundary, BytesFn},
    engine::{self, Status},
};
use shellfox_ghostty_vt::{Selection, ViewportPoint};
use std::ffi::c_void;

#[repr(C)]
pub struct SfmSelection {
    pub kind: u32,
    pub start_x: u16,
    pub start_y: u16,
    pub end_x: u16,
    pub end_y: u16,
    pub rectangle: u8,
}
pub(crate) fn scroll(handle: u64, rows: i32) -> engine::Result<()> {
    if !(-1_000_000..=1_000_000).contains(&rows) {
        return Err(Status::Invalid);
    }
    engine::with_engine(handle, |owned| {
        owned.terminal.scroll(rows as isize).map_err(map_error)
    })
}
pub(crate) fn set_selection(handle: u64, selection: Option<Selection>) -> engine::Result<()> {
    engine::with_engine(handle, |owned| {
        owned.terminal.set_selection(selection).map_err(map_error)
    })
}
pub(crate) fn copy_selection(handle: u64) -> engine::Result<Option<String>> {
    engine::with_engine(handle, |owned| {
        owned.terminal.copy_selection().map_err(map_error)
    })
}
pub(crate) fn hyperlink_at(handle: u64, point: ViewportPoint) -> engine::Result<Option<String>> {
    engine::with_engine(handle, |owned| {
        owned.terminal.hyperlink_at(point).map_err(map_error)
    })
}
fn file_link_at(handle: u64, point: ViewportPoint) -> engine::Result<Option<String>> {
    engine::with_engine(handle, |owned| {
        owned.terminal.file_link_at(point).map_err(map_error)
    })
}
pub(crate) fn mouse_tracking(handle: u64) -> engine::Result<bool> {
    engine::with_engine(handle, |owned| {
        owned.terminal.mouse_tracking().map_err(map_error)
    })
}
/// Wheel steps go to the application: mouse reports, or cursor keys in the
/// alternate screen.
fn wheel_reporting(handle: u64) -> engine::Result<bool> {
    engine::with_engine(handle, |owned| {
        let terminal = &mut owned.terminal;
        Ok(terminal.mouse_tracking().map_err(map_error)?
            || terminal.alternate_scroll().map_err(map_error)?)
    })
}
#[no_mangle]
pub extern "C" fn sfm_terminal_scroll(handle: u64, rows: i32) -> i32 {
    boundary(|| scroll(handle, rows))
}
#[no_mangle]
pub extern "C" fn sfm_terminal_select(handle: u64, selection: SfmSelection) -> i32 {
    boundary(|| {
        if selection.rectangle > 1 {
            return Err(Status::Invalid);
        }
        let start = ViewportPoint {
            x: selection.start_x,
            y: selection.start_y,
        };
        let end = ViewportPoint {
            x: selection.end_x,
            y: selection.end_y,
        };
        let selected = match selection.kind {
            0 => None,
            1 => Some(Selection::Range {
                start,
                end,
                rectangle: selection.rectangle == 1,
            }),
            2 => Some(Selection::Word(start)),
            3 => Some(Selection::Line(start)),
            4 => Some(Selection::All),
            _ => return Err(Status::Invalid),
        };
        set_selection(handle, selected)
    })
}
fn map_error(error: shellfox_ghostty_vt::Error) -> Status {
    if error.0 == -2 {
        Status::Invalid
    } else {
        Status::Engine
    }
}
#[no_mangle]
pub extern "C" fn sfm_terminal_copy(
    handle: u64,
    callback: Option<BytesFn>,
    context: *mut c_void,
) -> i32 {
    boundary(|| {
        let callback = callback.ok_or(Status::Invalid)?;
        let text = copy_selection(handle)?.unwrap_or_default();
        callback(context, text.as_ptr(), text.len());
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn sfm_terminal_hyperlink(
    handle: u64,
    x: u16,
    y: u16,
    callback: Option<BytesFn>,
    context: *mut c_void,
) -> i32 {
    boundary(|| {
        let callback = callback.ok_or(Status::Invalid)?;
        let uri = hyperlink_at(handle, ViewportPoint { x, y })?.unwrap_or_default();
        callback(context, uri.as_ptr(), uri.len());
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn sfm_terminal_file_link(
    handle: u64,
    x: u16,
    y: u16,
    callback: Option<BytesFn>,
    context: *mut c_void,
) -> i32 {
    boundary(|| {
        let callback = callback.ok_or(Status::Invalid)?;
        let target = file_link_at(handle, ViewportPoint { x, y })?.unwrap_or_default();
        callback(context, target.as_ptr(), target.len());
        Ok(())
    })
}
/// # Safety
/// out must point to one writable byte.
#[no_mangle]
pub unsafe extern "C" fn sfm_terminal_mouse_tracking(handle: u64, out: *mut u8) -> i32 {
    boundary(|| {
        if out.is_null() {
            return Err(Status::Invalid);
        }
        let tracking = mouse_tracking(handle)?;
        // SAFETY: caller promises writable storage.
        unsafe {
            out.write(u8::from(tracking));
        }
        Ok(())
    })
}
/// # Safety
/// out must point to one writable byte.
#[no_mangle]
pub unsafe extern "C" fn sfm_terminal_wheel_reporting(handle: u64, out: *mut u8) -> i32 {
    boundary(|| {
        if out.is_null() {
            return Err(Status::Invalid);
        }
        let reporting = wheel_reporting(handle)?;
        // SAFETY: caller promises writable storage.
        unsafe {
            out.write(u8::from(reporting));
        }
        Ok(())
    })
}
