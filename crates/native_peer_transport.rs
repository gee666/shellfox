//! Five closed v2 operations through the already-running local Service and its
//! retained authenticated Go session. No socket, helper, selection or retry here.
use shellfox_protocol::{
    AuthenticatedPeerId, Endpoint, OutboundTransport, RequestLimits, TransportError,
    TransportFuture,
};
use shellfox_service::{
    local::LocalClient,
    native::NativeClient,
    remote::{OutgoingV2Endpoint, OutgoingV2Future},
};
/// Internal native/headless owner adaptation, not a network protocol.
pub trait ServiceOwner: Send + Sync {
    fn outgoing<'a>(
        &'a self,
        peer: &'a AuthenticatedPeerId,
        endpoint: OutgoingV2Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> OutgoingV2Future<'a>;
}
impl ServiceOwner for NativeClient {
    fn outgoing<'a>(
        &'a self,
        peer: &'a AuthenticatedPeerId,
        endpoint: OutgoingV2Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> OutgoingV2Future<'a> {
        Box::pin(self.outgoing_v2_with_status(peer, endpoint, body, limits))
    }
}
impl ServiceOwner for &LocalClient {
    fn outgoing<'a>(
        &'a self,
        peer: &'a AuthenticatedPeerId,
        endpoint: OutgoingV2Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> OutgoingV2Future<'a> {
        Box::pin(self.outgoing_v2_with_status(peer, endpoint, body, limits))
    }
}
pub struct ServiceTransport<C>(C);
impl<C: ServiceOwner> ServiceTransport<C> {
    pub fn new(owner: C) -> Self {
        Self(owner)
    }
}
#[cfg(test)]
#[path = "native_peer_transport_tests.rs"]
mod tests;
impl<C: ServiceOwner> OutboundTransport for ServiceTransport<C> {
    fn send<'a>(
        &'a self,
        peer: &'a AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> TransportFuture<'a> {
        Box::pin(async move {
            let endpoint = match endpoint {
                Endpoint::DiscoverV2 => OutgoingV2Endpoint::Discover,
                Endpoint::PairV2 => OutgoingV2Endpoint::Pair,
                Endpoint::RequestV2 => OutgoingV2Endpoint::Request,
                Endpoint::EventsV2 => OutgoingV2Endpoint::Events,
                Endpoint::DisconnectV2 => OutgoingV2Endpoint::Disconnect,
                _ => return Err(TransportError::InvalidRequest),
            };
            let response = self.0.outgoing(peer, endpoint, body, limits).await?;
            // Actual HTTP status only. A business-code/message mentioning 404 is
            // never a transport rejection or a legacy-capability hint.
            if !(200..300).contains(&response.status) {
                return Err(TransportError::Rejected {
                    status: response.status,
                });
            }
            Ok(response.body)
        })
    }
}
