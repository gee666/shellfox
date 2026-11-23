use super::*;

fn fence_native(mobile: &Mobile) -> Result<()> {
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    if s.closed {
        return Err(STALE);
    }
    s.generation = s.generation.checked_add(1).ok_or(BUSY)?;
    for token in s.operations.values() {
        token.cancel.cancel();
    }
    let pending: Vec<_> = s.requests.drain().collect();
    s.outbound.clear();
    for (id, request) in pending {
        if request.dispatched {
            s.cancels.push_back((request.generation, id));
        }
    }
    terminal::invalidate(&mut s)?;
    mobile.client.suspend();
    Ok(())
}

/// # Safety
/// Inputs are readable UTF-8 spans; out points to writable u64 storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_client_create(
    peer: *const u8,
    peer_len: usize,
    identity: *const u8,
    identity_len: usize,
    out: *mut u64,
) -> i32 {
    boundary(|| unsafe {
        output(out, || {
            let peer = AuthenticatedPeerId::new(text(peer, peer_len, 200)?).map_err(|_| INVALID)?;
            let identity = text(identity, identity_len, 36)?
                .parse::<ClientId>()
                .map_err(|_| INVALID)?;
            create(peer, identity)
        })
    })
}
/// # Safety
/// out points to writable u64 storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_client_connect(handle: u64, out: *mut u64) -> i32 {
    boundary(|| unsafe { output(out, || operations::pair(registry_lookup(handle)?)) })
}
/// # Safety
/// out points to writable u64 storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_client_reconnect(handle: u64, out: *mut u64) -> i32 {
    boundary(|| unsafe {
        output(out, || {
            let mobile = registry_lookup(handle)?;
            let _dispatch = mobile.dispatch_gate.lock().map_err(|_| PANIC)?;
            fence_native(&mobile)?;
            mobile.client.reconnect(ClientId::new_v4());
            operations::pair(mobile.clone())
        })
    })
}
/// # Safety
/// out points to writable u64 storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_client_disconnect(handle: u64, out: *mut u64) -> i32 {
    boundary(|| unsafe {
        output(out, || {
            let mobile = registry_lookup(handle)?;
            let _dispatch = mobile.dispatch_gate.lock().map_err(|_| PANIC)?;
            fence_native(&mobile)?;
            launch(
                mobile.clone(),
                CompletionOrigin::Explicit,
                |mobile| async move {
                    let result = mobile.client.disconnect().await.map_err(client_error)?;
                    serde_json::to_value(result).map_err(|_| PROTOCOL)
                },
            )
        })
    })
}
/// # Safety
/// Callback and context must obey the C header. Context is never retained.
#[no_mangle]
pub unsafe extern "C" fn sfm_client_close(
    handle: u64,
    cancel: Option<CancelFn>,
    context: *mut c_void,
) -> i32 {
    boundary(|| {
        let mobile = registry_lookup(handle)?;
        let _dispatch = mobile.dispatch_gate.lock().map_err(|_| PANIC)?;
        let cancel = cancel.ok_or(INVALID)?;
        let cancellations = {
            let mut s = mobile.state.lock().map_err(|_| PANIC)?;
            if s.closed {
                return Err(STALE);
            }
            s.closed = true;
            mobile.waker.set(handle, None, std::ptr::null_mut());
            for token in s.operations.values() {
                token.cancel.cancel();
            }
            let pending: Vec<_> = s.requests.drain().collect();
            for (id, p) in pending {
                if p.dispatched {
                    s.cancels.push_back((p.generation, id));
                }
            }
            s.outbound.clear();
            s.events.clear();
            s.event_bytes = 0;
            s.bindings.clear();
            s.cancels.drain(..).collect::<Vec<_>>()
        };
        mobile.client.suspend();
        remove(handle)?;
        terminal::release(&mobile);
        for (generation, request) in cancellations {
            unsafe {
                cancel(context, handle, generation, request);
            }
        }
        Ok(())
    })
}
