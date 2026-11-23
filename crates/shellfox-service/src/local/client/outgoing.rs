use super::*;
use crate::remote::{outgoing, outgoing_wire, PeerResponse};
use shellfox_protocol::AuthenticatedPeerId;
use shellfox_protocol::Endpoint;
impl LocalClient {
    /// Same originating private Host lifetime, original Go peer/session, bounded
    /// status/body. Never starts networking, pairs, retries, or accepts a URL.
    pub async fn peer_request_with_status(
        &self,
        peer: &AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> std::result::Result<PeerResponse, TransportError> {
        let limits = outgoing::validate(&body, limits)?;
        if self
            .lifetime
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none()
        {
            return Err(TransportError::Unavailable);
        }
        let payload = outgoing_wire::Request::new(peer, endpoint, body, &limits)?;
        let envelope = Envelope {
            client_id: self.transport.client_id,
            method: outgoing_wire::METHOD.into(),
            payload,
        };
        let encoded = serde_json::to_vec(&envelope).map_err(|_| TransportError::InvalidRequest)?;
        if encoded.len() > outgoing_wire::MAX_REQUEST {
            return Err(TransportError::TooLarge);
        }
        // Base64 chunks avoid Vec<u8>'s numeric-JSON amplification. Only this
        // typed operation has the expanded private framing budget; peer bounds
        // remain the ordinary protocol request/response caps.
        let maximum = limits.max_response_bytes;
        let wire_limits = RequestLimits {
            max_request_bytes: outgoing_wire::MAX_REQUEST,
            max_response_bytes: outgoing_wire::MAX_RESPONSE,
            ..limits
        };
        let received = self.transport.send(encoded, wire_limits).await?;
        let reply: shellfox_protocol::Reply<outgoing_wire::Reply> =
            serde_json::from_slice(&received).map_err(|_| TransportError::InvalidResponse)?;
        match reply {
            shellfox_protocol::Reply::Success(Ok(response)) => response.into_http(maximum),
            shellfox_protocol::Reply::Success(Err(error)) => Err(error.into()),
            shellfox_protocol::Reply::Failure(_) => Err(TransportError::Unavailable),
        }
    }
}
