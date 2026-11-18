use super::*;

/// # Safety
/// tab references tab_len readable UTF-8 bytes.
#[no_mangle]
pub unsafe extern "C" fn sfm_client_bind_terminal(
    handle: u64,
    tab: *const u8,
    tab_len: usize,
    engine: u64,
) -> i32 {
    boundary(|| {
        let mobile = registry_lookup(handle)?;
        // SAFETY: native supplies readable tab UUID bytes.
        let tab = unsafe { text(tab, tab_len, 36)? }
            .parse()
            .map_err(|_| INVALID)?;
        terminal::bind(&mobile, tab, engine)
    })
}
#[no_mangle]
pub extern "C" fn sfm_client_unbind_terminal(handle: u64, engine: u64) -> i32 {
    boundary(|| terminal::unbind(registry_lookup(handle)?.as_ref(), engine))
}
#[no_mangle]
pub extern "C" fn sfm_client_terminal_reset_done(handle: u64, engine: u64, ticket: u64) -> i32 {
    boundary(|| terminal::reset_done(registry_lookup(handle)?.as_ref(), engine, ticket))
}
/// # Safety
/// out points to writable u64 storage. Query on the terminal owner thread before reset.
#[no_mangle]
pub unsafe extern "C" fn sfm_client_terminal_reset_ticket(
    handle: u64,
    engine: u64,
    out: *mut u64,
) -> i32 {
    boundary(|| unsafe {
        output(out, || {
            terminal::reset_ticket(registry_lookup(handle)?.as_ref(), engine)
        })
    })
}
/// # Safety
/// cols and rows point to writable uint16_t storage. Inputs identify a live reset ticket.
#[no_mangle]
pub unsafe extern "C" fn sfm_client_terminal_reset_grid(
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
        let (width, height) =
            terminal::reset_grid(registry_lookup(handle)?.as_ref(), engine, ticket)?;
        unsafe {
            cols.write(width);
            rows.write(height);
        }
        Ok(())
    })
}
/// # Safety
/// out points to writable uint8_t storage. This only reads native consumption.
#[no_mangle]
pub unsafe extern "C" fn sfm_client_terminal_recovery_ready(
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
        let ready = terminal::ready_handle(registry_lookup(handle)?.as_ref(), engine)?;
        unsafe {
            out.write(u8::from(ready));
        }
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn sfm_client_terminal_consumed(handle: u64, engine: u64) -> i32 {
    boundary(|| {
        let mobile = registry_lookup(handle)?;
        let ack = terminal::consumed(&mobile, engine)?;
        let connection = mobile.legacy()?.state().connection_id;
        let retry = ack.clone();
        let owner = mobile.clone();
        if let Err(error) = launch_small(
            mobile,
            CompletionOrigin::NativeInternal,
            move |mobile| async move {
                let result = match request_slot(&mobile).await {
                    Ok(_slot) => mobile.legacy()?.execute(&ack).await.map_err(client_error),
                    Err(error) => Err(error),
                };
                terminal::ack_finished(
                    &mobile,
                    &ack,
                    connection,
                    matches!(result, Ok(shellfox_protocol::Reply::Success(_))),
                );
                serde_json::to_value(result?).map_err(|_| PROTOCOL)
            },
        ) {
            terminal::ack_finished(&owner, &retry, connection, false);
            return Err(error);
        }
        Ok(())
    })
}
/// # Safety
/// ptr references len readable UTF-8 bytes; out points to writable u64 storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_client_terminal_write(
    handle: u64,
    engine: u64,
    ptr: *const u8,
    len: usize,
    out: *mut u64,
) -> i32 {
    // SAFETY: the additive entry point uses the same borrowed span/out contract.
    unsafe { sfm_client_terminal_write_ex(handle, engine, ptr, len, 1, out) }
}
/// # Safety
/// ptr references len readable UTF-8 bytes; out points to writable u64 storage.
/// user_initiated must be the validated Rust input callback hint, either 0 or 1.
#[no_mangle]
pub unsafe extern "C" fn sfm_client_terminal_write_ex(
    handle: u64,
    engine: u64,
    ptr: *const u8,
    len: usize,
    user_initiated: u8,
    out: *mut u64,
) -> i32 {
    boundary(|| unsafe {
        output(out, || {
            let user_initiated = match user_initiated {
                0 => false,
                1 => true,
                _ => return Err(INVALID),
            };
            if observer_bound(engine) {
                return Err(INVALID);
            }
            let mobile = registry_lookup(handle)?;
            let data = text(ptr, len, crate::MAX_INPUT)?.to_owned();
            let request = terminal::written(&mobile, engine, data, user_initiated)?;
            mutations::enqueue(
                mobile,
                RemoteCommand::WriteTerminal(request),
                CompletionOrigin::NativeInternal,
            )
        })
    })
}
/// # Safety
/// out points to writable u64 storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_client_terminal_resize(
    handle: u64,
    engine: u64,
    cols: u16,
    rows: u16,
    out: *mut u64,
) -> i32 {
    boundary(|| unsafe {
        output(out, || {
            if observer_bound(engine) {
                return Err(INVALID);
            }
            let mobile = registry_lookup(handle)?;
            let request = terminal::resized(&mobile, engine, cols, rows)?;
            mutations::enqueue(
                mobile,
                RemoteCommand::ResizeTerminal(request),
                CompletionOrigin::NativeInternal,
            )
        })
    })
}
