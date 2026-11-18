use super::{
    state::{CancelOnDrop, ClientError},
    CommonClient, Profile,
};
use serde::{de::DeserializeOwned, Serialize};
use shellfox_protocol::*;
use std::time::{Duration, Instant};
impl<T: OutboundTransport, P: Profile> CommonClient<T, P> {
    pub(super) async fn send<B: Serialize, R: DeserializeOwned + Validate>(
        &self,
        endpoint: Endpoint,
        body: &B,
        cancel: CancellationToken,
    ) -> Result<R, ClientError<P::Error>> {
        let bytes = serde_json::to_vec(body).map_err(|_| ClientError::InvalidRequest)?;
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err(ClientError::InvalidRequest);
        }
        if cancel.is_cancelled() {
            return Err(ClientError::Transport(TransportError::Cancelled));
        }
        let child = cancel.child_token();
        let _guard = CancelOnDrop(child.clone());
        let limits = RequestLimits {
            deadline: Instant::now() + Duration::from_secs(30),
            cancel: child.clone(),
            max_request_bytes: MAX_REQUEST_BYTES,
            max_response_bytes: MAX_RESPONSE_BYTES,
        };
        let bytes = self
            .transport
            .send(&self.peer, endpoint, bytes, limits.clone())
            .await
            .map_err(ClientError::Transport)?;
        if cancel.is_cancelled() || child.is_cancelled() {
            return Err(ClientError::Transport(TransportError::Cancelled));
        }
        if Instant::now() >= limits.deadline {
            return Err(ClientError::Transport(TransportError::Deadline));
        }
        decode(&bytes).map_err(|_| ClientError::InvalidResponse)
    }
}
