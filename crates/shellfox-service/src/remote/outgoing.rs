//! Opaque outgoing HTTP only. Pairing/selection belong to the shared SDK client.
use shellfox_protocol::{
    AuthenticatedPeerId, CancellationToken, Endpoint, RequestLimits, TransportError,
};
pub use shellfox_transport::HttpResponse as PeerResponse;
use std::{
    future::Future,
    pin::Pin,
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

pub type PeerFuture<'a> =
    Pin<Box<dyn Future<Output = Result<PeerResponse, TransportError>> + Send + 'a>>;
pub(crate) struct OutgoingEnvelope {
    pub peer: AuthenticatedPeerId,
    pub endpoint: Endpoint,
    pub body: Vec<u8>,
    pub limits: RequestLimits,
    pub reply: oneshot::Sender<Result<PeerResponse, TransportError>>,
}
pub(crate) fn validate(
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
    limits.max_request_bytes = limits
        .max_request_bytes
        .min(shellfox_protocol::MAX_REQUEST_BYTES);
    limits.max_response_bytes = limits
        .max_response_bytes
        .min(shellfox_protocol::MAX_RESPONSE_BYTES);
    if body.len() > limits.max_request_bytes {
        return Err(TransportError::TooLarge);
    }
    serde_json::from_slice::<serde_json::Value>(body)
        .map_err(|_| TransportError::InvalidRequest)?;
    Ok(limits)
}
impl super::native::RemoteControl {
    pub(crate) fn outgoing_deadline(&self, deadline: Instant) -> tokio::time::Sleep {
        // GPUI polls native requests outside Tokio. Only timer creation needs
        // to enter the runtime that owns the remote manager.
        let _runtime = self.runtime.enter();
        tokio::time::sleep_until(deadline.into())
    }

    pub(crate) async fn peer_request(
        &self,
        peer: &AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> Result<PeerResponse, TransportError> {
        let limits = validate(&body, limits)?;
        let cancel = limits.cancel.clone();
        let deadline = limits.deadline;
        let (reply, receive) = oneshot::channel();
        let envelope = OutgoingEnvelope {
            peer: peer.clone(),
            endpoint,
            body,
            limits,
            reply,
        };
        // Waiting for queue space is backpressure, bounded by the caller's deadline.
        let exchange = async {
            self.outgoing
                .send(envelope)
                .await
                .map_err(|_| TransportError::Unavailable)?;
            receive.await.unwrap_or(Err(TransportError::Unavailable))
        };
        tokio::select! {
            biased;
            _=cancel.cancelled()=>Err(TransportError::Cancelled),
            _=self.outgoing_deadline(deadline)=>Err(TransportError::Deadline),
            result=exchange=>result,
        }
    }
}
pub(crate) struct OutgoingFence {
    pub peer: String,
    pub cancel: CancellationToken,
}
