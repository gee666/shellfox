//! Native remote client ABI. See include/shellfox_mobile_client.h for ownership.
#[path = "client_lifecycle.rs"]
mod lifecycle;
#[path = "client_mutations.rs"]
mod mutations;
#[path = "client_observer.rs"]
mod observer;
#[path = "client_observer_actions.rs"]
mod observer_actions;
#[path = "client_observer_delivery.rs"]
mod observer_delivery;
#[path = "client_observer_events.rs"]
mod observer_events;
#[path = "client_observer_flow.rs"]
mod observer_flow;
#[path = "client_observer_http.rs"]
mod observer_http;
#[path = "client_observer_owner.rs"]
mod observer_owner;
#[path = "client_operations.rs"]
mod operations;
#[path = "client_profile.rs"]
mod profile;
pub use observer::*;
pub(crate) use profile::observer_bound;
#[path = "client_runtime.rs"]
mod runtime;
#[path = "client_terminal.rs"]
mod terminal;
#[path = "client_terminal_abi.rs"]
mod terminal_abi;
#[path = "client_transport.rs"]
mod transport;
#[path = "client_uploads.rs"]
mod uploads;
#[path = "client_waker.rs"]
mod waker;
pub use waker::WakeFn;
pub use lifecycle::*;
pub use terminal_abi::*;
#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
use runtime::*;
use shellfox_protocol::{AuthenticatedPeerId, ClientId, RemoteCommand};
use std::{
    ffi::c_void,
    panic::{catch_unwind, AssertUnwindSafe},
    slice,
};

pub type RequestFn = unsafe extern "C" fn(
    *mut c_void,
    u64,
    u64,
    u64,
    *const u8,
    usize,
    *const u8,
    usize,
    *const u8,
    usize,
    u32,
    usize,
);
pub type CancelFn = unsafe extern "C" fn(*mut c_void, u64, u64, u64);
fn boundary(action: impl FnOnce() -> Result<()>) -> i32 {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(Ok(())) => 0,
        Ok(Err(code)) => code,
        Err(_) => PANIC,
    }
}
unsafe fn bytes<'a>(ptr: *const u8, len: usize, max: usize) -> Result<&'a [u8]> {
    if len > max || len > isize::MAX as usize || (len > 0 && ptr.is_null()) {
        return Err(INVALID);
    }
    if len == 0 {
        return Ok(&[]);
    }
    // SAFETY: caller supplies readable memory for the duration of the ABI call.
    Ok(unsafe { slice::from_raw_parts(ptr, len) })
}
unsafe fn text<'a>(ptr: *const u8, len: usize, max: usize) -> Result<&'a str> {
    // SAFETY: same borrowed memory contract as bytes.
    std::str::from_utf8(unsafe { bytes(ptr, len, max)? }).map_err(|_| INVALID)
}
unsafe fn output(out: *mut u64, action: impl FnOnce() -> Result<u64>) -> Result<()> {
    if out.is_null() {
        return Err(INVALID);
    }
    // SAFETY: native caller provides writable u64 storage.
    unsafe {
        out.write(0);
        out.write(action()?);
    }
    Ok(())
}
#[no_mangle]
pub extern "C" fn sfm_client_abi_version() -> u32 {
    1
}
/// # Safety
/// ptr references len readable bytes; out points to writable u64 storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_client_operation(
    handle: u64,
    ptr: *const u8,
    len: usize,
    out: *mut u64,
) -> i32 {
    boundary(|| unsafe {
        output(out, || {
            let command: RemoteCommand =
                serde_json::from_slice(bytes(ptr, len, shellfox_protocol::MAX_REQUEST_BYTES)?)
                    .map_err(|_| INVALID)?;
            let mobile = registry_lookup(handle)?;
            operations::accept(mobile, command)
        })
    })
}
/// # Safety
/// out points to writable u64 storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_client_poll(handle: u64, out: *mut u64) -> i32 {
    boundary(|| unsafe { output(out, || operations::poll(registry_lookup(handle)?)) })
}
#[no_mangle]
pub extern "C" fn sfm_client_cancel(handle: u64, operation: u64) -> i32 {
    boundary(|| {
        let mobile = registry_lookup(handle)?;
        let s = mobile.state.lock().map_err(|_| PANIC)?;
        s.operations.get(&operation).ok_or(STALE)?.cancel.cancel();
        Ok(())
    })
}
/// # Safety
/// `wake` and `context` must stay callable from any thread until replaced,
/// cleared with NULL, or the client is closed. See the C header.
#[no_mangle]
pub unsafe extern "C" fn sfm_client_set_waker(
    handle: u64,
    wake: Option<WakeFn>,
    context: *mut c_void,
) -> i32 {
    boundary(|| {
        let mobile = registry_lookup(handle)?;
        // Close clears the waker under the same lock, so none survives it.
        let s = mobile.state.lock().map_err(|_| PANIC)?;
        if s.closed {
            return Err(STALE);
        }
        mobile.waker.set(handle, wake, context);
        drop(s);
        Ok(())
    })
}
/// # Safety
/// Callbacks must obey the borrowed-pointer/no-unwind contract in the C header.
#[no_mangle]
pub unsafe extern "C" fn sfm_client_dispatch(
    handle: u64,
    request: Option<RequestFn>,
    cancel: Option<CancelFn>,
    context: *mut c_void,
) -> i32 {
    boundary(|| {
        let mobile = registry_lookup(handle)?;
        let _dispatch = mobile.dispatch_gate.lock().map_err(|_| PANIC)?;
        mobile.waker.rearm();
        transport::dispatch(
            &mobile,
            request.ok_or(INVALID)?,
            cancel.ok_or(INVALID)?,
            context,
        )
    })
}
/// # Safety
/// ptr references len readable bytes and is borrowed only until return.
#[no_mangle]
pub unsafe extern "C" fn sfm_client_complete(
    handle: u64,
    generation: u64,
    request: u64,
    outcome: i32,
    ptr: *const u8,
    len: usize,
) -> i32 {
    boundary(|| {
        let mobile = registry_lookup(handle)?;
        mobile.legacy()?;
        // SAFETY: forwarded native memory contract, checked before copying.
        let bytes = unsafe { bytes(ptr, len, shellfox_protocol::MAX_RESPONSE_BYTES)? };
        transport::complete(&mobile, generation, request, outcome, bytes)
    })
}
/// # Safety
/// needed is writable; buffer is writable for capacity bytes, or NULL at zero.
#[no_mangle]
pub unsafe extern "C" fn sfm_client_read_event(
    handle: u64,
    buffer: *mut u8,
    capacity: usize,
    needed: *mut usize,
) -> i32 {
    boundary(|| {
        if needed.is_null() || capacity > isize::MAX as usize || (capacity > 0 && buffer.is_null())
        {
            return Err(INVALID);
        }
        // SAFETY: caller provides writable usize storage.
        unsafe {
            needed.write(0);
        }
        let mobile = registry_lookup(handle)?;
        let mut s = mobile.state.lock().map_err(|_| PANIC)?;
        if s.closed {
            return Err(STALE);
        }
        let event = s.events.front().ok_or(EMPTY)?;
        unsafe {
            needed.write(event.len());
        }
        if event.len() > capacity {
            return Err(BUFFER);
        }
        // SAFETY: capacity was checked; destination is caller-owned and cannot alias the private event.
        unsafe {
            std::ptr::copy_nonoverlapping(event.as_ptr(), buffer, event.len());
        }
        s.event_bytes -= event.len();
        s.events.pop_front();
        Ok(())
    })
}
