//! Request and response bounds for the helper proxy.
use crate::{RequestLimits, TransportError, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES};
use std::time::{Duration, Instant};

/// Clamps the caller's limits and rejects a request before any helper I/O.
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
/// Host approval and revocation errors pass through byte-for-byte.
pub(crate) fn validate_response(bytes: Vec<u8>, limit: usize) -> Result<Vec<u8>, TransportError> {
    if bytes.len() > limit {
        return Err(TransportError::TooLarge);
    }
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .map_err(|_| TransportError::InvalidResponse)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CancellationToken;

    fn limits() -> RequestLimits {
        RequestLimits {
            deadline: Instant::now() + Duration::from_secs(30),
            cancel: CancellationToken::new(),
            max_request_bytes: MAX_REQUEST_BYTES,
            max_response_bytes: MAX_RESPONSE_BYTES,
        }
    }
    #[test]
    fn requests_are_bounded_before_any_io() {
        let cancelled = limits();
        cancelled.cancel.cancel();
        assert_eq!(
            validate_request(b"{}", cancelled).err(),
            Some(TransportError::Cancelled)
        );
        let mut expired = limits();
        expired.deadline = Instant::now();
        assert_eq!(
            validate_request(b"{}", expired).err(),
            Some(TransportError::Deadline)
        );
        assert_eq!(
            validate_request(b"not-json", limits()).err(),
            Some(TransportError::InvalidRequest)
        );
        assert_eq!(
            validate_request(&vec![b' '; MAX_REQUEST_BYTES + 1], limits()).err(),
            Some(TransportError::TooLarge)
        );
        let mut expanded = limits();
        expanded.max_request_bytes = usize::MAX;
        expanded.max_response_bytes = usize::MAX;
        expanded.deadline += Duration::from_secs(60);
        let clamped = validate_request(b"{}", expanded).unwrap();
        assert_eq!(clamped.max_request_bytes, MAX_REQUEST_BYTES);
        assert_eq!(clamped.max_response_bytes, MAX_RESPONSE_BYTES);
        assert!(clamped.deadline <= Instant::now() + Duration::from_secs(30));
    }
    #[test]
    fn responses_must_be_bounded_json() {
        assert_eq!(
            validate_response(b"invalid".to_vec(), MAX_RESPONSE_BYTES).err(),
            Some(TransportError::InvalidResponse)
        );
        assert_eq!(
            validate_response(vec![b' '; 3], 2).err(),
            Some(TransportError::TooLarge)
        );
        let denial = br#"{"ok":false,"error":{"code":"AUTH_FAILED"}}"#.to_vec();
        assert_eq!(
            validate_response(denial.clone(), MAX_RESPONSE_BYTES).unwrap(),
            denial
        );
    }
    #[tokio::test]
    async fn cancellation_and_deadline_drop_the_inflight_future() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let never = std::future::pending::<Result<(), TransportError>>();
        let far = Instant::now() + Duration::from_secs(30);
        assert_eq!(
            bounded(never, far, &cancel).await,
            Err(TransportError::Cancelled)
        );
        let never = std::future::pending::<Result<(), TransportError>>();
        let soon = Instant::now() + Duration::from_millis(10);
        assert_eq!(
            bounded(never, soon, &CancellationToken::new()).await,
            Err(TransportError::Deadline)
        );
    }
}
