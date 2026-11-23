use super::*;
use crate::remote::{operator, RemoteOperatorState, RemoteRequest};
impl LocalClient {
    /// Authenticated LOCAL operator control of this exact retained Host. This
    /// cannot set a control URL, install a provider, or implicitly PAIR. Explicit
    /// Configure uses the original Manager's existing owned node lifecycle.
    /// Accepted lifecycle/approval work survives a lost reply; never replay it.
    pub async fn remote_control(&self, request: RemoteRequest) -> Result<RemoteOperatorState> {
        request.validate().map_err(|error| LocalRequestFailure {
            method: operator::METHOD,
            error,
        })?;
        if self
            .lifetime
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none()
        {
            bail!("Original local operator lifetime is closed");
        }
        let envelope = Envelope {
            client_id: self.transport.client_id,
            method: operator::METHOD.into(),
            payload: request,
        };
        let bytes = serde_json::to_vec(&envelope)?;
        if bytes.len() > shellfox_protocol::MAX_REQUEST_BYTES {
            bail!("Remote operator request exceeds bound");
        }
        let limits = RequestLimits {
            deadline: Instant::now() + Duration::from_secs(30),
            cancel: shellfox_protocol::CancellationToken::new(),
            max_request_bytes: shellfox_protocol::MAX_REQUEST_BYTES,
            max_response_bytes: shellfox_protocol::MAX_RESPONSE_BYTES,
        };
        let received=self.transport.send(bytes,limits).await.map_err(|error|anyhow::anyhow!("Remote operator transport failed ({error:?}); accepted outcome may be unknown and was not retried"))?;
        let reply: shellfox_protocol::Reply<operator::Reply> = serde_json::from_slice(&received)?;
        match reply {
            shellfox_protocol::Reply::Success(Ok(state)) => {
                operator::validate_state(&state)?;
                Ok(state)
            }
            shellfox_protocol::Reply::Success(Err(error))
            | shellfox_protocol::Reply::Failure(error) => Err(LocalRequestFailure {
                method: operator::METHOD,
                error,
            }
            .into()),
        }
    }
}
