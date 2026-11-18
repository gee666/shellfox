use crate::{
    AuthenticatedPeerId, Endpoint, OutboundTransport, RequestLimits, TransportError,
    TransportFuture, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES,
};
use std::time::{Duration, Instant};

/// Outbound-only native Go/gomobile binding. Implementations must cancel native
/// work when the future drops, cap allocations before returning, never follow
/// redirects and never replay a POST. No host secret, subprocess or local PTY API.
pub trait NativeOutbound: Send + Sync {
    fn request<'a>(
        &'a self,
        peer: &'a AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> TransportFuture<'a>;
}

pub struct NativeAdapter<T>(T);
impl<T: NativeOutbound> NativeAdapter<T> {
    pub fn new(native: T) -> Self {
        Self(native)
    }
}
impl<T: NativeOutbound> OutboundTransport for NativeAdapter<T> {
    fn send<'a>(
        &'a self,
        peer: &'a AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> TransportFuture<'a> {
        Box::pin(async move {
            let limits = validate_request(&body, limits)?;
            let response_limit = limits.max_response_bytes;
            let cancel = limits.cancel.clone();
            let deadline = limits.deadline;
            let future = self.0.request(peer, endpoint, body, limits);
            let bytes = bounded(future, deadline, &cancel).await?;
            validate_response(bytes, response_limit)
        })
    }
}

pub(crate) fn validate_request(
    body: &[u8],
    mut limits: RequestLimits,
) -> Result<RequestLimits, TransportError> {
    if limits.cancel.is_cancelled() {
        return Err(TransportError::Cancelled);
    }
    let now = Instant::now();
    if limits.deadline <= now {
        return Err(TransportError::Deadline);
    }
    limits.deadline = limits.deadline.min(now + Duration::from_secs(30));
    limits.max_request_bytes = limits.max_request_bytes.min(MAX_REQUEST_BYTES);
    limits.max_response_bytes = limits.max_response_bytes.min(MAX_RESPONSE_BYTES);
    if body.len() > limits.max_request_bytes {
        return Err(TransportError::TooLarge);
    }
    serde_json::from_slice::<serde_json::Value>(body)
        .map_err(|_| TransportError::InvalidRequest)?;
    Ok(limits)
}
pub(crate) fn validate_response(bytes: Vec<u8>, limit: usize) -> Result<Vec<u8>, TransportError> {
    if bytes.len() > limit {
        return Err(TransportError::TooLarge);
    }
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|_| TransportError::InvalidResponse)?;
    // Preserve host approval/revocation errors byte-for-byte for the shared client.
    Ok(bytes)
}
pub(crate) async fn bounded<F, T>(
    future: F,
    deadline: Instant,
    cancel: &crate::CancellationToken,
) -> Result<T, TransportError>
where
    F: std::future::Future<Output = Result<T, TransportError>>,
{
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(TransportError::Cancelled),
        result = tokio::time::timeout_at(deadline.into(), future) => result.map_err(|_| TransportError::Deadline)?,
    }
}
