use crate::{AuthenticatedPeerId, Endpoint};
use std::{future::Future, pin::Pin, time::Instant};

pub use tokio_util::sync::CancellationToken;

/// Transport must enforce the deadline and cancellation even while reading.
/// Limits include the entire encoded body, not just terminal data.
#[derive(Clone, Debug)]
pub struct RequestLimits {
    pub deadline: Instant,
    pub cancel: CancellationToken,
    pub max_request_bytes: usize,
    pub max_response_bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportError {
    Cancelled,
    Deadline,
    Unavailable,
    TooLarge,
    Authentication,
    InvalidRequest,
    InvalidResponse,
    Rejected { status: u16 },
}

pub type TransportFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<u8>, TransportError>> + Send + 'a>>;

/// Native transport authenticates the remote peer. No URL or caller identity is accepted.
/// A failed send has an uncertain outcome and must never be retried as a mutation.
pub trait OutboundTransport: Send + Sync {
    fn send<'a>(
        &'a self,
        peer: &'a AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> TransportFuture<'a>;
}
