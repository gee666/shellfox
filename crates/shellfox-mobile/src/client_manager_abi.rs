//! Explicit Manager-v2 native ABI. No default selector or caller ACK capability.
use super::*;
use crate::client::{boundary, bytes, output, runtime::*, text};
#[no_mangle]
pub extern "C" fn sfm_manager_v2_abi_version() -> u32 {
    1
}
/// # Safety
/// UTF-8 inputs are borrowed spans; out points to writable u64 storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_manager_v2_create(
    peer: *const u8,
    peer_len: usize,
    identity: *const u8,
    identity_len: usize,
    out: *mut u64,
) -> i32 {
    boundary(|| unsafe {
        output(out, || {
            let peer = AuthenticatedPeerId::new(text(peer, peer_len, 200)?).map_err(|_| INVALID)?;
            let id = text(identity, identity_len, 36)?
                .parse()
                .map_err(|_| INVALID)?;
            create_manager(peer, id)
        })
    })
}
/// # Safety
/// out points to writable u64 storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_manager_v2_discover(handle: u64, out: *mut u64) -> i32 {
    boundary(|| unsafe {
        output(out, || {
            let mobile = registry_lookup(handle)?;
            mobile.manager()?;
            launch(mobile, CompletionOrigin::Explicit, |mobile| async move {
                let result = mobile
                    .manager()?
                    .discover(shellfox_client::manager_v2::FallbackPolicy::Never)
                    .await
                    .map_err(super::operations::client_error)?;
                Ok(match result {
                    shellfox_client::manager_v2::DiscoveryOutcome::Available(value) => {
                        serde_json::json!({"kind":"available","discovery":value})
                    }
                    shellfox_client::manager_v2::DiscoveryOutcome::Incompatible(value) => {
                        serde_json::json!({"kind":"incompatible","discovery":value})
                    }
                    shellfox_client::manager_v2::DiscoveryOutcome::LegacyAvailable => {
                        return Err(PROTOCOL)
                    }
                })
            })
        })
    })
}
/// # Safety
/// attempt is a readable UUID span; out points to writable u64 storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_manager_v2_pair(
    handle: u64,
    attempt: *const u8,
    len: usize,
    out: *mut u64,
) -> i32 {
    boundary(|| unsafe {
        output(out, || {
            let attempt = text(attempt, len, 36)?
                .parse::<m::Id>()
                .map_err(|_| INVALID)?;
            let mobile = registry_lookup(handle)?;
            mobile.manager()?;
            launch(
                mobile,
                CompletionOrigin::Explicit,
                move |mobile| async move {
                    serde_json::to_value(
                        mobile
                            .manager()?
                            .pair(attempt)
                            .await
                            .map_err(super::operations::client_error)?,
                    )
                    .map_err(|_| PROTOCOL)
                },
            )
        })
    })
}
/// # Safety
/// identity is a readable UUID span. This local fence does not send requests.
#[no_mangle]
pub unsafe extern "C" fn sfm_manager_v2_reconnect(
    handle: u64,
    identity: *const u8,
    len: usize,
) -> i32 {
    boundary(|| {
        let id = unsafe { text(identity, len, 36)? }
            .parse::<m::Id>()
            .map_err(|_| INVALID)?;
        let mobile = registry_lookup(handle)?;
        let _dispatch = mobile.dispatch_gate.lock().map_err(|_| PANIC)?;
        mobile.manager()?.reconnect(id);
        super::bindings::fence(&mobile)
    })
}
/// # Safety
/// out points to writable u64 storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_manager_v2_disconnect(handle: u64, out: *mut u64) -> i32 {
    boundary(|| unsafe {
        output(out, || {
            let mobile = registry_lookup(handle)?;
            mobile.manager()?;
            let _dispatch = mobile.dispatch_gate.lock().map_err(|_| PANIC)?;
            super::bindings::fence(&mobile)?;
            launch(
                mobile.clone(),
                CompletionOrigin::Explicit,
                move |mobile| async move {
                    serde_json::to_value(
                        mobile
                            .manager()?
                            .disconnect()
                            .await
                            .map_err(super::operations::client_error)?,
                    )
                    .map_err(|_| PROTOCOL)
                },
            )
        })
    })
}
/// # Safety
/// json is a readable canonical manager command. out is writable u64 storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_manager_v2_operation(
    handle: u64,
    json: *const u8,
    len: usize,
    out: *mut u64,
) -> i32 {
    boundary(|| unsafe {
        output(out, || {
            let command = serde_json::from_slice::<m::Command>(bytes(
                json,
                len,
                shellfox_protocol::MAX_REQUEST_BYTES,
            )?)
            .map_err(|_| INVALID)?;
            super::operations::command(registry_lookup(handle)?, command)
        })
    })
}
/// # Safety
/// out points to writable u64 storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_manager_v2_poll(handle: u64, out: *mut u64) -> i32 {
    boundary(|| unsafe { output(out, || super::operations::poll(registry_lookup(handle)?)) })
}
/// # Safety
/// tab is a readable UUID span. The engine belongs to the calling native owner.
#[no_mangle]
pub unsafe extern "C" fn sfm_manager_v2_bind_terminal(
    handle: u64,
    tab: *const u8,
    len: usize,
    engine: u64,
) -> i32 {
    boundary(|| {
        let tab = unsafe { text(tab, len, 36)? }
            .parse::<m::Id>()
            .map_err(|_| INVALID)?;
        super::bindings::bind(registry_lookup(handle)?.as_ref(), tab, engine)
    })
}
#[no_mangle]
pub extern "C" fn sfm_manager_v2_unbind_terminal(handle: u64, engine: u64) -> i32 {
    boundary(|| super::bindings::unbind(registry_lookup(handle)?.as_ref(), engine))
}
/// # Safety
/// out points to writable u8 storage; called only by the terminal owner.
#[no_mangle]
pub unsafe extern "C" fn sfm_manager_v2_terminal_recovery_ready(
    handle: u64,
    engine: u64,
    out: *mut u8,
) -> i32 {
    boundary(|| {
        if out.is_null() {
            return Err(INVALID);
        }
        unsafe { out.write(0) };
        let b = super::bindings::by_engine(registry_lookup(handle)?.as_ref(), engine)?;
        if b.shared.owner != std::thread::current().id() {
            return Err(crate::Status::WrongThread as i32);
        }
        unsafe {
            out.write(u8::from(b.ready()?));
        }
        Ok(())
    })
}
#[no_mangle]
pub extern "C" fn sfm_manager_v2_terminal_consumed(handle: u64, engine: u64) -> i32 {
    boundary(|| super::bindings::consumed(registry_lookup(handle)?, engine))
}
/// # Safety
/// Input is validated native input JSON, never a control ref/ACK. out is u64 storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_manager_v2_terminal_input(
    handle: u64,
    engine: u64,
    json: *const u8,
    len: usize,
    user_initiated: u8,
    out: *mut u64,
) -> i32 {
    boundary(|| unsafe {
        output(out, || {
            let hint = match user_initiated {
                0 => false,
                1 => true,
                _ => return Err(INVALID),
            };
            let mobile=registry_lookup(handle)?;
            let b=super::bindings::by_engine(&mobile,engine)?;
            if b.shared.owner!=std::thread::current().id(){return Err(crate::Status::WrongThread as i32);}
            let Some((input,hint))=super::native_input::project(&mobile,engine,bytes(json,len,shellfox_protocol::MAX_REQUEST_BYTES)?,hint)?else{return Ok(0)};
            super::input::enqueue(mobile, engine, input, hint)
        })
    })
}
/// # Safety
/// out points to writable u64 storage; called by the terminal owner.
#[no_mangle]
pub unsafe extern "C" fn sfm_manager_v2_terminal_resize(
    handle: u64,
    engine: u64,
    cols: u16,
    rows: u16,
    out: *mut u64,
) -> i32 {
    boundary(|| unsafe {
        output(out, || {
            super::input::resize(registry_lookup(handle)?, engine, cols, rows)
        })
    })
}
/// # Safety
/// Body span is copied before return. Uses actual HTTP status, not JSON guesses.
#[no_mangle]
pub unsafe extern "C" fn sfm_manager_v2_complete_http(
    handle: u64,
    generation: u64,
    request: u64,
    outcome: i32,
    status: u16,
    data: *const u8,
    len: usize,
) -> i32 {
    boundary(|| {
        let mobile = registry_lookup(handle)?;
        mobile.manager()?;
        crate::client::observer_http::complete_v2(
            &mobile,
            generation,
            request,
            outcome,
            status,
            unsafe { bytes(data, len, shellfox_protocol::MAX_RESPONSE_BYTES)? },
        )
    })
}
