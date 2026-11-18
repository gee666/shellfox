use crate::{
    outbound::{bounded, validate_request},
    AuthenticatedPeerId, Endpoint, HelperTransport, RequestLimits, TransportError,
    MAX_REQUEST_BYTES,
};

/// Received final HTTP status and bounded body. Neither field is synthesized.
#[derive(Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}
impl std::fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("body_bytes", &self.body.len())
            .finish()
    }
}
impl HelperTransport {
    /// Same retained helper/proxy and one POST as the legacy adapter. No retry,
    /// redirect, caller URL/header, or replacement helper is admitted.
    pub async fn request_with_status(
        &self,
        peer: &AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> Result<HttpResponse, TransportError> {
        self.request_http(peer, endpoint, body, limits, false).await
    }
    pub(crate) async fn request_http(
        &self,
        peer: &AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
        reject_non_success: bool,
    ) -> Result<HttpResponse, TransportError> {
        let limits = validate_request(&body, limits)?;
        if self.stopped.is_cancelled() {
            return Err(TransportError::Unavailable);
        }
        let mut envelope =
            serde_json::to_vec(&serde_json::json!({"peerId":peer.as_str(),"path":endpoint.path()}))
                .map_err(|_| TransportError::InvalidRequest)?;
        envelope.pop();
        envelope.extend_from_slice(b",\"body\":");
        envelope.extend_from_slice(&body);
        envelope.push(b'}');
        if envelope.len() > limits.max_request_bytes.min(MAX_REQUEST_BYTES) {
            return Err(TransportError::TooLarge);
        }
        let future = async {
            tokio::select! {
                biased;
                _ = self.stopped.cancelled() => Err(TransportError::Unavailable),
                response = crate::http::post_with_status(self.proxy.address, self.proxy_secret.expose(), &envelope, limits.max_response_bytes, reject_non_success) => response,
            }
        };
        bounded(future, limits.deadline, &limits.cancel).await
    }
}
