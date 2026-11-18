use crate::{
    engine::{self, Result, Status},
    frame::{self, FrameFn},
    input, MAX_CHUNK, MAX_INPUT,
};
use std::{
    ffi::{c_char, c_void},
    panic::{catch_unwind, AssertUnwindSafe},
    slice,
};

pub type BytesFn = extern "C" fn(*mut c_void, *const u8, usize);
pub type SfmInputExFn = extern "C" fn(*mut c_void, *const u8, usize, u8);
pub type ErrorFn = extern "C" fn(*mut c_void, i32);
#[repr(C)]
#[derive(Debug, Default)]
pub struct SfmProgress {
    pub accepted_sequence: u64,
    pub consumed_sequence: u64,
    pub queued_bytes: u32,
    pub queued_chunks: u32,
}
pub(crate) fn boundary(action: impl FnOnce() -> Result<()>) -> i32 {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(Ok(())) => Status::Ok as i32,
        Ok(Err(status)) => status as i32,
        Err(_) => Status::Panic as i32,
    }
}
pub(crate) unsafe fn bytes<'a>(ptr: *const u8, length: usize, max: usize) -> Result<&'a [u8]> {
    if length == 0 || length > max || ptr.is_null() {
        return Err(Status::Invalid);
    }
    // SAFETY: caller provides readable memory for length, as required by the C contract.
    Ok(unsafe { slice::from_raw_parts(ptr, length) })
}
#[no_mangle]
pub extern "C" fn sfm_abi_version() -> u32 {
    1
}
#[no_mangle]
pub extern "C" fn sfm_status_message(status: i32) -> *const c_char {
    match status {
        0 => c"ok",
        1 => c"mobile queue busy",
        2 => c"invalid mobile input",
        3 => c"stale mobile handle",
        4 => c"wrong terminal thread",
        5 => c"terminal engine failed",
        _ => c"mobile binding failed",
    }
    .as_ptr()
}
/// # Safety
/// out must point to writable storage for a u64.
#[no_mangle]
pub unsafe extern "C" fn sfm_terminal_create(cols: u16, rows: u16, out: *mut u64) -> i32 {
    boundary(|| {
        if out.is_null() {
            return Err(Status::Invalid);
        }
        // SAFETY: checked non-null; caller promises writable storage.
        unsafe {
            out.write(0);
            out.write(engine::create(cols, rows)?);
        }
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn sfm_terminal_close(handle: u64) -> i32 {
    boundary(|| engine::close(handle))
}
/// # Safety
/// ptr must reference length readable bytes for the duration of this call.
#[no_mangle]
pub unsafe extern "C" fn sfm_terminal_submit(
    handle: u64,
    sequence: u64,
    ptr: *const u8,
    length: usize,
) -> i32 {
    boundary(|| {
        let shared = engine::lookup(handle)?;
        // SAFETY: forwarded C memory contract.
        let data = unsafe { bytes(ptr, length, MAX_CHUNK)? };
        let mut queue = shared.queue.lock().map_err(|_| Status::Panic)?;
        #[cfg(feature = "experimental-shellfox-native")]
        if shared
            .passive_manager
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(Status::Invalid);
        }
        queue.submit(sequence, data)
    })
}
/// # Safety
/// out must point to writable SfmProgress storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_terminal_progress(handle: u64, out: *mut SfmProgress) -> i32 {
    boundary(|| {
        if out.is_null() {
            return Err(Status::Invalid);
        }
        let shared = engine::lookup(handle)?;
        let q = shared.queue.lock().map_err(|_| Status::Panic)?;
        // SAFETY: caller promises writable storage.
        unsafe {
            out.write(SfmProgress {
                accepted_sequence: q.accepted,
                consumed_sequence: q.consumed,
                queued_bytes: q.bytes as u32,
                queued_chunks: q.chunks.len() as u32,
            });
        }
        if q.failed {
            return Err(Status::Engine);
        }
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn sfm_terminal_pump(
    handle: u64,
    error: Option<ErrorFn>,
    context: *mut c_void,
) -> i32 {
    let status = boundary(|| engine::with_engine(handle, engine::pump));
    if status != 0 {
        if let Some(callback) = error {
            callback(context, status);
        }
    }
    status
}
#[no_mangle]
pub extern "C" fn sfm_terminal_frame(
    handle: u64,
    callback: Option<FrameFn>,
    context: *mut c_void,
) -> i32 {
    boundary(|| {
        let callback = callback.ok_or(Status::Invalid)?;
        engine::with_engine(handle, |owned| frame::frame(owned, callback, context))
    })
}
#[no_mangle]
pub extern "C" fn sfm_terminal_resize(handle: u64, cols: u16, rows: u16) -> i32 {
    boundary(|| {
        if crate::client::observer_bound(handle) {
            return Err(Status::Invalid);
        }
        engine::dimensions(cols, rows)?;
        engine::with_engine(handle, |owned| {
            owned
                .terminal
                .resize(cols, rows)
                .map_err(|_| Status::Engine)?;
            owned.terminal.drain_replies().map_err(|_| Status::Engine)?;
            Ok(())
        })
    })
}
#[no_mangle]
pub extern "C" fn sfm_terminal_reset(handle: u64, cols: u16, rows: u16) -> i32 {
    boundary(|| engine::reset(handle, cols, rows))
}
/// # Safety
/// json references length readable bytes. The callback must not throw or retain
/// borrowed bytes. context is opaque and passed back only during this call.
#[no_mangle]
pub unsafe extern "C" fn sfm_terminal_input_ex(
    handle: u64,
    json: *const u8,
    length: usize,
    callback: Option<SfmInputExFn>,
    context: *mut c_void,
) -> i32 {
    boundary(|| {
        let callback = callback.ok_or(Status::Invalid)?;
        // SAFETY: forwarded caller memory contract, bounded before parsing.
        let json = unsafe { bytes(json, length, MAX_INPUT)? };
        let (encoded, user_initiated) = encode_input(handle, json)?;
        // encode_input returned owned bytes. All engine borrows and queue locks
        // have been released before native FIFO admission runs in the callback.
        if !encoded.is_empty() {
            callback(context, encoded.as_ptr(), encoded.len(), user_initiated);
        }
        Ok(())
    })
}

pub(crate) fn encode_input(handle: u64, json: &[u8]) -> Result<(Vec<u8>, u8)> {
    if crate::client::observer_bound(handle) {
        return Err(Status::Invalid);
    }
    let event = serde_json::from_slice::<input::Input>(json).map_err(|_| Status::Invalid)?;
    let user_initiated = event.user_initiated();
    let encoded = engine::with_engine(handle, |owned| input::encode(&mut owned.terminal, event))?;
    let user_initiated = u8::from(user_initiated && !encoded.is_empty());
    Ok((encoded, user_initiated))
}

/// # Safety
/// ptr must reference length readable bytes. The callback must not throw or retain
/// its borrowed bytes. context is opaque and only passed back to that callback.
#[no_mangle]
pub unsafe extern "C" fn sfm_terminal_input(
    handle: u64,
    ptr: *const u8,
    length: usize,
    callback: Option<BytesFn>,
    context: *mut c_void,
) -> i32 {
    boundary(|| {
        let callback = callback.ok_or(Status::Invalid)?;
        // SAFETY: forwarded C memory contract.
        let json = unsafe { bytes(ptr, length, MAX_INPUT)? };
        let (encoded, _) = encode_input(handle, json)?;
        // Native input forwarding may inspect the terminal or enter client FIFO
        // admission. No engine borrow or queue lock may survive into its callback.
        if !encoded.is_empty() {
            callback(context, encoded.as_ptr(), encoded.len());
        }
        Ok(())
    })
}
