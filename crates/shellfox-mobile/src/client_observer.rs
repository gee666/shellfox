pub(super) use super::observer_delivery::DeliveryBinding;
pub(super) use super::observer_events::finish_event;
pub use super::observer_owner::*;
use super::*;
use shellfox_client::observer_v2::{
    ClientError as ObserverError, DiscoveryOutcome, FallbackPolicy,
};
use shellfox_protocol::observer_v2 as v2;

pub(super) fn observer_error(error: ObserverError) -> i32 {
    match error {
        ObserverError::InvalidRequest | ObserverError::ProfileDenied => INVALID,
        ObserverError::InvalidResponse | ObserverError::Host(_) => PROTOCOL,
        ObserverError::Transport(shellfox_protocol::TransportError::Cancelled) => CANCELLED,
        ObserverError::Transport(_) => NETWORK,
        ObserverError::Busy => BUSY,
        ObserverError::NotConnected
        | ObserverError::NotDiscovered
        | ObserverError::StaleConnection => STALE,
    }
}
fn available(mobile: &Mobile) -> Result<()> {
    mobile.observer()?;
    let s = mobile.state.lock().map_err(|_| PANIC)?;
    if s.closed || s.observer_withdrawing {
        Err(STALE)
    } else {
        Ok(())
    }
}
#[no_mangle]
pub extern "C" fn sfm_observer_v2_abi_version() -> u32 {
    1
}
/// # Safety
/// profile and capabilities identify writable uint32_t storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_client_profile(
    handle: u64,
    profile: *mut u32,
    capabilities: *mut u32,
) -> i32 {
    boundary(|| {
        if profile.is_null() || capabilities.is_null() {
            return Err(INVALID);
        }
        unsafe {
            profile.write(0);
            capabilities.write(0);
        }
        let mobile = registry_lookup(handle)?;
        let (id, apis) = if mobile.is_manager() {
            (3, 7)
        } else if mobile.is_observer() {
            (2, 1)
        } else {
            (1, 7)
        };
        unsafe {
            profile.write(id);
            capabilities.write(apis);
        }
        Ok(())
    })
}
/// # Safety
/// Input spans are readable UTF-8; out is writable uint64_t storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_observer_v2_create(
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
                .parse::<v2::Id>()
                .map_err(|_| INVALID)?;
            create_observer(peer, identity)
        })
    })
}
/// # Safety
/// out is writable uint64_t storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_observer_v2_discover(
    handle: u64,
    offer_legacy: u8,
    out: *mut u64,
) -> i32 {
    boundary(|| unsafe {
        output(out, || {
            let policy = match offer_legacy {
                0 => FallbackPolicy::Never,
                1 => FallbackPolicy::OfferLegacy,
                _ => return Err(INVALID),
            };
            let mobile = registry_lookup(handle)?;
            available(&mobile)?;
            launch(
                mobile,
                CompletionOrigin::Explicit,
                move |mobile| async move {
                    match mobile.observer()?.discover(policy).await {
                        Ok(DiscoveryOutcome::Available(value)) => {
                            Ok(serde_json::json!({"kind":"available","discovery":value}))
                        }
                        Ok(DiscoveryOutcome::Incompatible(value)) => {
                            Ok(serde_json::json!({"kind":"incompatible","discovery":value}))
                        }
                        Ok(DiscoveryOutcome::LegacyAvailable) => {
                            Ok(serde_json::json!({"kind":"legacyAvailable"}))
                        }
                        Err(ObserverError::Host(error)) => {
                            serde_json::to_value(v2::Reply::<v2::Discovery>::Failure(error))
                                .map_err(|_| PROTOCOL)
                        }
                        Err(error) => Err(observer_error(error)),
                    }
                },
            )
        })
    })
}
/// # Safety
/// attempt is a readable UTF-8 UUID span; out is writable uint64_t storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_observer_v2_pair(
    handle: u64,
    attempt: *const u8,
    len: usize,
    out: *mut u64,
) -> i32 {
    boundary(|| unsafe {
        output(out, || {
            let attempt = text(attempt, len, 36)?
                .parse::<v2::Id>()
                .map_err(|_| INVALID)?;
            let mobile = registry_lookup(handle)?;
            available(&mobile)?;
            launch(
                mobile,
                CompletionOrigin::Explicit,
                move |mobile| async move {
                    let result = mobile
                        .observer()?
                        .pair(attempt)
                        .await
                        .map_err(observer_error)?;
                    serde_json::to_value(result).map_err(|_| PROTOCOL)
                },
            )
        })
    })
}
/// # Safety
/// json is readable for len bytes; out is writable uint64_t storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_observer_v2_operation(
    handle: u64,
    json: *const u8,
    len: usize,
    out: *mut u64,
) -> i32 {
    boundary(|| unsafe {
        output(out, || {
            let command: v2::Command =
                serde_json::from_slice(bytes(json, len, shellfox_protocol::MAX_REQUEST_BYTES)?)
                    .map_err(|_| INVALID)?;
            let mobile = registry_lookup(handle)?;
            available(&mobile)?;
            super::observer_flow::command(mobile, command)
        })
    })
}
/// # Safety
/// out is writable uint64_t storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_observer_v2_poll(handle: u64, out: *mut u64) -> i32 {
    boundary(|| unsafe { output(out, || super::observer_flow::poll(registry_lookup(handle)?)) })
}
/// # Safety
/// identity is a readable UUID span. This local fence starts no requests.
#[no_mangle]
pub unsafe extern "C" fn sfm_observer_v2_reconnect(
    handle: u64,
    identity: *const u8,
    len: usize,
) -> i32 {
    boundary(|| {
        let identity = unsafe { text(identity, len, 36)? }
            .parse::<v2::Id>()
            .map_err(|_| INVALID)?;
        let mobile = registry_lookup(handle)?;
        let client = mobile.observer()?;
        let _dispatch = mobile.dispatch_gate.lock().map_err(|_| PANIC)?;
        let mut s = mobile.state.lock().map_err(|_| PANIC)?;
        super::observer_actions::fence(&mut s)?;
        s.observer_withdrawing = false;
        s.observer_disconnect = None;
        client.reconnect(identity);
        Ok(())
    })
}
/// # Safety
/// out is writable uint64_t storage.
#[no_mangle]
pub unsafe extern "C" fn sfm_observer_v2_disconnect(handle: u64, out: *mut u64) -> i32 {
    boundary(|| unsafe {
        output(out, || {
            super::observer_actions::disconnect(registry_lookup(handle)?)
        })
    })
}
/// # Safety
/// bytes are readable for len bytes; all data is copied before return.
#[no_mangle]
pub unsafe extern "C" fn sfm_observer_v2_complete_http(
    handle: u64,
    generation: u64,
    request: u64,
    outcome: i32,
    http_status: u16,
    ptr: *const u8,
    len: usize,
) -> i32 {
    boundary(|| {
        let mobile = registry_lookup(handle)?;
        mobile.observer()?;
        let bytes = unsafe { bytes(ptr, len, shellfox_protocol::MAX_RESPONSE_BYTES)? };
        super::observer_http::complete(&mobile, generation, request, outcome, http_status, bytes)
    })
}
