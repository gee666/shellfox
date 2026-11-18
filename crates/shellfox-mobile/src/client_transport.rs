use super::{runtime::*, CancelFn, RequestFn};
use shellfox_protocol::*;
use std::{
    ffi::c_void,
    sync::Weak,
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

pub(super) struct NativeTransport(pub Weak<Mobile>);
pub(super) struct Pending {
    pub generation: u64,
    pub dispatched: bool,
    pub operation: CancellationToken,
    pub connection: CancellationToken,
    pub peer: String,
    pub endpoint: Endpoint,
    pub body: Vec<u8>,
    pub deadline: Instant,
    pub max_response: usize,
    pub reply: oneshot::Sender<std::result::Result<Vec<u8>, TransportError>>,
}
struct Flight {
    mobile: Weak<Mobile>,
    request: u64,
}
impl Drop for Flight {
    fn drop(&mut self) {
        if let Some(mobile) = self.mobile.upgrade() {
            if let Ok(mut s) = mobile.state.lock() {
                if let Some(pending) = s.requests.remove(&self.request) {
                    s.outbound.retain(|id| *id != self.request);
                    if pending.dispatched {
                        s.cancels.push_back((pending.generation, self.request));
                    }
                }
            }
        }
    }
}
impl OutboundTransport for NativeTransport {
    fn send<'a>(
        &'a self,
        peer: &'a AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> TransportFuture<'a> {
        Box::pin(async move {
            if body.is_empty() || body.len() > limits.max_request_bytes.min(MAX_REQUEST_BYTES) {
                return Err(TransportError::InvalidRequest);
            }
            let mobile = self.0.upgrade().ok_or(TransportError::Cancelled)?;
            let operation = CURRENT
                .try_with(Clone::clone)
                .map_err(|_| TransportError::InvalidRequest)?;
            let (request, receive) = {
                let mut s = mobile
                    .state
                    .lock()
                    .map_err(|_| TransportError::Unavailable)?;
                if s.closed
                    || s.generation != operation.generation
                    || operation.cancel.is_cancelled()
                    || limits.cancel.is_cancelled()
                {
                    return Err(TransportError::Cancelled);
                }
                if s.requests.len() >= 32 || s.cancels.len() >= 64 {
                    return Err(TransportError::Unavailable);
                }
                let request = s.next_request;
                s.next_request = request.checked_add(1).ok_or(TransportError::Unavailable)?;
                let (reply, receive) = oneshot::channel();
                let generation = s.generation;
                s.requests.insert(
                    request,
                    Pending {
                        generation,
                        dispatched: false,
                        operation: operation.cancel.clone(),
                        connection: limits.cancel.clone(),
                        peer: peer.as_str().to_owned(),
                        endpoint,
                        body,
                        deadline: limits.deadline,
                        max_response: limits.max_response_bytes.min(MAX_RESPONSE_BYTES),
                        reply,
                    },
                );
                s.outbound.push_back(request);
                (request, receive)
            };
            let _flight = Flight {
                mobile: self.0.clone(),
                request,
            };
            let deadline = tokio::time::Instant::from_std(limits.deadline);
            tokio::select! {
                biased;
                _ = operation.cancel.cancelled() => Err(TransportError::Cancelled),
                _ = limits.cancel.cancelled() => Err(TransportError::Cancelled),
                _ = tokio::time::sleep_until(deadline) => Err(TransportError::Deadline),
                value = receive => value.unwrap_or(Err(TransportError::Unavailable)),
            }
        })
    }
}
pub(super) fn complete(
    mobile: &Mobile,
    generation: u64,
    request: u64,
    outcome: i32,
    bytes: &[u8],
) -> Result<()> {
    let outcome = match outcome {
        0 => None,
        1 => Some(TransportError::Cancelled),
        2 => Some(TransportError::Deadline),
        3 => Some(TransportError::Unavailable),
        4 => Some(TransportError::TooLarge),
        5 => Some(TransportError::Authentication),
        6 => Some(TransportError::InvalidResponse),
        _ => return Err(INVALID),
    };
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    if s.closed || s.generation != generation {
        return Err(STALE);
    }
    let pending = s.requests.get(&request).ok_or(STALE)?;
    if pending.generation != generation
        || !pending.dispatched
        || pending.operation.is_cancelled()
        || pending.connection.is_cancelled()
    {
        return Err(STALE);
    }
    let result = if Instant::now() >= pending.deadline {
        Err(TransportError::Deadline)
    } else if bytes.len() > pending.max_response {
        Err(TransportError::TooLarge)
    } else if let Some(error) = outcome {
        Err(error)
    } else {
        Ok(bytes.to_vec())
    };
    let pending = s.requests.remove(&request).ok_or(STALE)?;
    pending.reply.send(result).map_err(|_| STALE)
}
/// No callbacks or contexts are retained. Each invocation owns its borrowed data.
pub(super) fn dispatch(
    mobile: &Mobile,
    request_fn: RequestFn,
    cancel_fn: CancelFn,
    context: *mut c_void,
) -> Result<()> {
    for _ in 0..32 {
        let cancel = mobile.state.lock().map_err(|_| PANIC)?.cancels.pop_front();
        if let Some((generation, request)) = cancel {
            // Function pointers follow the C header; native must not unwind.
            unsafe {
                cancel_fn(context, mobile.id, generation, request);
            }
            continue;
        }
        let next = {
            let mut s = mobile.state.lock().map_err(|_| PANIC)?;
            if s.closed {
                return Err(STALE);
            }
            let Some(id) = s.outbound.pop_front() else {
                break;
            };
            s.requests
                .get_mut(&id)
                .filter(|p| !p.operation.is_cancelled() && !p.connection.is_cancelled())
                .map(|p| {
                    p.dispatched = true;
                    (
                        id,
                        p.generation,
                        p.peer.clone(),
                        p.endpoint.path(),
                        p.body.clone(),
                        p.deadline.saturating_duration_since(Instant::now()),
                        p.max_response,
                    )
                })
        };
        if let Some((id, generation, peer, path, body, remaining, max_response)) = next {
            let millis = remaining
                .max(Duration::from_millis(1))
                .as_millis()
                .min(u128::from(u32::MAX)) as u32;
            unsafe {
                request_fn(
                    context,
                    mobile.id,
                    generation,
                    id,
                    peer.as_ptr(),
                    peer.len(),
                    path.as_ptr(),
                    path.len(),
                    body.as_ptr(),
                    body.len(),
                    millis,
                    max_response,
                );
            }
        }
    }
    Ok(())
}
