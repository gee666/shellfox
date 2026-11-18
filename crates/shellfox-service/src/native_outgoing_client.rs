use super::NativeClient;
use crate::remote::{OutgoingV2Endpoint, OutgoingV2Response};
use shellfox_protocol::{AuthenticatedPeerId, RequestLimits, TransportError};
impl NativeClient {
    /// One opaque POST through this Host's original Go session. No implicit
    /// PAIR/selection; the shared SDK owns those across all views.
    pub async fn outgoing_v2_with_status(
        &self,
        peer: &AuthenticatedPeerId,
        endpoint: OutgoingV2Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> Result<OutgoingV2Response, TransportError> {
        self.owner
            .outgoing_v2_with_status(peer, endpoint, body, limits, None)
            .await
    }
}
