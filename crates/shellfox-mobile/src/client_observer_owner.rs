use super::{observer_delivery as delivery, runtime::*, *};
use shellfox_protocol::observer_v2 as v2;

pub(super) fn grid(engine: u64) -> Result<(u16, u16)> {
    crate::engine::with_engine(engine, |owned| {
        let screen = owned.terminal.render().map_err(|_| crate::Status::Engine)?;
        Ok((screen.cols, screen.rows))
    })
    .map_err(|e| e as i32)
}
fn reset_done(mobile: &Mobile, engine: u64, ticket: u64) -> Result<()> {
    let current = mobile.observer()?.binding();
    let shared = crate::engine::lookup(engine).map_err(|e| e as i32)?;
    if shared.owner != std::thread::current().id() {
        return Err(crate::Status::WrongThread as i32);
    }
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    if s.closed {
        return Err(STALE);
    }
    let b = s
        .observers
        .values_mut()
        .find(|b| b.terminal == engine)
        .ok_or(STALE)?;
    if !b.reset_required || b.reset_ticket != ticket {
        return Err(STALE);
    }
    let q = shared.queue.lock().map_err(|_| PANIC)?;
    if q.closed || q.failed || q.accepted != 0 || q.consumed != 0 || !q.chunks.is_empty() {
        return Err(BUSY);
    }
    drop(q);
    if let Some(expected) = b.required_grid {
        if grid(engine)? != expected {
            return Err(INVALID);
        }
    }
    if let Some(cursor) = b.cursor.as_ref() {
        if current.as_ref() != Some(cursor.binding()) {
            return Err(STALE);
        }
        let bytes = cursor.recovery_bytes().map_err(|_| PROTOCOL)?;
        b.pending.clear();
        b.tickets.clear();
        b.recovery_ticket = None;
        for chunk in bytes.chunks(crate::MAX_CHUNK) {
            b.pending.push_back(delivery::Segment {
                bytes: chunk.to_vec(),
                sequence: None,
            });
        }
        delivery::flush(b)?;
    }
    b.reset_required = false;
    Ok(())
}
/// # Safety
/// tab is a readable UUID span, never a caller claim of input/layout authority.
#[no_mangle]
pub unsafe extern "C" fn sfm_observer_v2_bind_terminal(
    handle: u64,
    tab: *const u8,
    len: usize,
    engine: u64,
) -> i32 {
    boundary(|| {
        let tab = unsafe { text(tab, len, 36)? }
            .parse::<v2::Id>()
            .map_err(|_| INVALID)?;
        delivery::bind(registry_lookup(handle)?.as_ref(), tab, engine)
    })
}
#[no_mangle]
pub extern "C" fn sfm_observer_v2_unbind_terminal(handle: u64, engine: u64) -> i32 {
    boundary(|| delivery::unbind(registry_lookup(handle)?.as_ref(), engine))
}
/// # Safety
/// out is writable uint64_t storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_observer_v2_terminal_reset_ticket(
    handle: u64,
    engine: u64,
    out: *mut u64,
) -> i32 {
    boundary(|| unsafe {
        output(out, || {
            let mobile = registry_lookup(handle)?;
            mobile.observer()?;
            let s = mobile.state.lock().map_err(|_| PANIC)?;
            if s.closed {
                return Err(STALE);
            }
            let b = s
                .observers
                .values()
                .find(|b| b.terminal == engine)
                .ok_or(STALE)?;
            if !b.reset_required {
                return Err(EMPTY);
            }
            Ok(b.reset_ticket)
        })
    })
}
/// # Safety
/// cols and rows are writable uint16_t storage, initialized to zero on failure.
#[no_mangle]
pub unsafe extern "C" fn sfm_observer_v2_terminal_reset_grid(
    handle: u64,
    engine: u64,
    ticket: u64,
    cols: *mut u16,
    rows: *mut u16,
) -> i32 {
    boundary(|| {
        if cols.is_null() || rows.is_null() {
            return Err(INVALID);
        }
        unsafe {
            cols.write(0);
            rows.write(0);
        }
        let mobile = registry_lookup(handle)?;
        mobile.observer()?;
        let s = mobile.state.lock().map_err(|_| PANIC)?;
        let b = s
            .observers
            .values()
            .find(|b| b.terminal == engine)
            .ok_or(STALE)?;
        if s.closed || !b.reset_required || b.reset_ticket != ticket {
            return Err(STALE);
        }
        let (width, height) = b.required_grid.ok_or(EMPTY)?;
        unsafe {
            cols.write(width);
            rows.write(height);
        }
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn sfm_observer_v2_terminal_reset_done(
    handle: u64,
    engine: u64,
    ticket: u64,
) -> i32 {
    boundary(|| reset_done(registry_lookup(handle)?.as_ref(), engine, ticket))
}
/// # Safety
/// out is writable uint8_t storage. Native recovery readiness is owner-thread only.
#[no_mangle]
pub unsafe extern "C" fn sfm_observer_v2_terminal_recovery_ready(
    handle: u64,
    engine: u64,
    out: *mut u8,
) -> i32 {
    boundary(|| {
        if out.is_null() {
            return Err(INVALID);
        }
        unsafe {
            out.write(0);
        }
        let mobile = registry_lookup(handle)?;
        mobile.observer()?;
        let shared = crate::engine::lookup(engine).map_err(|e| e as i32)?;
        if shared.owner != std::thread::current().id() {
            return Err(crate::Status::WrongThread as i32);
        }
        let s = mobile.state.lock().map_err(|_| PANIC)?;
        if s.closed {
            return Err(STALE);
        }
        let b = s
            .observers
            .values()
            .find(|b| b.terminal == engine)
            .ok_or(STALE)?;
        let ready = delivery::parsed(b)?;
        let actual = if ready { Some(grid(engine)?) } else { None };
        if ready && b.required_grid != actual {
            return Err(INVALID);
        }
        unsafe {
            out.write(u8::from(ready));
        }
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn sfm_observer_v2_terminal_consumed(handle: u64, engine: u64) -> i32 {
    boundary(|| super::observer_actions::consumed(registry_lookup(handle)?, engine))
}
