//! Opaque outgoing HTTP only. Pairing/selection belong to the shared SDK client.
use shellfox_protocol::{AuthenticatedPeerId, CancellationToken, RequestLimits, TransportError};
pub use shellfox_transport::HttpResponse as OutgoingV2Response;
use std::{
    future::Future,
    pin::Pin,
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutgoingV2Endpoint {
    Discover,
    Pair,
    Request,
    Events,
    Disconnect,
}
impl OutgoingV2Endpoint {
    pub(crate) fn wire(self) -> shellfox_protocol::Endpoint {
        use shellfox_protocol::Endpoint;
        match self {
            Self::Discover => Endpoint::DiscoverV2,
            Self::Pair => Endpoint::PairV2,
            Self::Request => Endpoint::RequestV2,
            Self::Events => Endpoint::EventsV2,
            Self::Disconnect => Endpoint::DisconnectV2,
        }
    }
}
pub type OutgoingV2Future<'a> =
    Pin<Box<dyn Future<Output = Result<OutgoingV2Response, TransportError>> + Send + 'a>>;
pub(crate) struct OutgoingEnvelope {
    pub peer: AuthenticatedPeerId,
    pub endpoint: OutgoingV2Endpoint,
    pub body: Vec<u8>,
    pub limits: RequestLimits,
    pub reply: oneshot::Sender<Result<OutgoingV2Response, TransportError>>,
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
    pub(crate) async fn outgoing_v2(
        &self,
        peer: &AuthenticatedPeerId,
        endpoint: OutgoingV2Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> Result<OutgoingV2Response, TransportError> {
        let limits = validate(&body, limits)?;
        let cancel = limits.cancel.clone();
        let deadline = limits.deadline;
        let (reply, receive) = oneshot::channel();
        self.outgoing
            .try_send(OutgoingEnvelope {
                peer: peer.clone(),
                endpoint,
                body,
                limits,
                reply,
            })
            .map_err(|_| TransportError::Unavailable)?;
        tokio::select! {
            biased;
            _=cancel.cancelled()=>Err(TransportError::Cancelled),
            _=tokio::time::sleep_until(deadline.into())=>Err(TransportError::Deadline),
            result=receive=>result.unwrap_or(Err(TransportError::Unavailable)),
        }
    }
}
pub(crate) struct OutgoingFence {
    pub peer: String,
    pub cancel: CancellationToken,
}
