use super::{runtime::*, transport};
use shellfox_protocol::{Endpoint, TransportError};
use std::time::Instant;

pub(super) fn complete(
    mobile: &Mobile,
    generation: u64,
    request: u64,
    outcome: i32,
    status: u16,
    bytes: &[u8],
) -> Result<()> {
    mobile.observer()?;
    complete_v2(mobile, generation, request, outcome, status, bytes)
}
pub(in crate::client) fn complete_v2(
    mobile: &Mobile,
    generation: u64,
    request: u64,
    outcome: i32,
    status: u16,
    bytes: &[u8],
) -> Result<()> {
    if outcome != 0 {
        if status != 0 || !bytes.is_empty() {
            return Err(INVALID);
        }
        return transport::complete(mobile, generation, request, outcome, bytes);
    }
    if !(100..=599).contains(&status) {
        return Err(INVALID);
    }
    if (200..=299).contains(&status) {
        return transport::complete(mobile, generation, request, 0, bytes);
    }
    if !bytes.is_empty() {
        return Err(INVALID);
    }
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    if s.closed || s.generation != generation {
        return Err(STALE);
    }
    let pending = s.requests.get(&request).ok_or(STALE)?;
    if !pending.dispatched
        || pending.generation != generation
        || pending.operation.is_cancelled()
        || pending.connection.is_cancelled()
    {
        return Err(STALE);
    }
    let error = if Instant::now() >= pending.deadline {
        TransportError::Deadline
    } else if status == 404 && pending.endpoint == Endpoint::DiscoverV2 {
        TransportError::Rejected { status: 404 }
    } else if status == 404 {
        TransportError::Unavailable
    } else if matches!(status, 401 | 403) {
        TransportError::Authentication
    } else {
        TransportError::Rejected { status }
    };
    s.requests
        .remove(&request)
        .ok_or(STALE)?
        .reply
        .send(Err(error))
        .map_err(|_| STALE)
}
