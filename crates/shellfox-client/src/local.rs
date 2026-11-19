use crate::ClientError;
use shellfox_protocol::*;
use std::time::{Duration, Instant};

/// Private local host only. This type and LocalRequest are absent from mobile defaults.
pub struct LocalClient<T> {
    client_id: ClientId,
    transport: T,
}
impl<T: LocalTransport> LocalClient<T> {
    pub fn new(client_id: ClientId, transport: T) -> Self {
        Self {
            client_id,
            transport,
        }
    }
    pub async fn execute<R: LocalRequest>(
        &self,
        request: &R,
    ) -> Result<Reply<R::Response>, ClientError> {
        self.execute_with_cancel(request, CancellationToken::new())
            .await
    }
    pub async fn execute_with_cancel<R: LocalRequest>(
        &self,
        request: &R,
        cancel: CancellationToken,
    ) -> Result<Reply<R::Response>, ClientError> {
        request.validate()?;
        const METHODS: &[&str] = &[
            LOCAL_NAMESPACE,
            "experimental.power.v1",
            DESKTOP_LAUNCH_NAMESPACE,
            DESKTOP_VIEW_NAMESPACE,
            SSH_READ_NAMESPACE,
            <automation::AutomationStart as LocalRequest>::METHOD,
            <automation::AutomationList as LocalRequest>::METHOD,
            <automation::AutomationRead as LocalRequest>::METHOD,
            <automation::AutomationWrite as LocalRequest>::METHOD,
            <automation::AutomationStop as LocalRequest>::METHOD,
            <automation::AutomationShow as LocalRequest>::METHOD,
        ];
        validation::ensure(METHODS.contains(&R::METHOD))?;
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Envelope<'a, R> {
            client_id: ClientId,
            method: &'static str,
            payload: &'a R,
        }
        let bytes = serde_json::to_vec(&Envelope {
            client_id: self.client_id,
            method: R::METHOD,
            payload: request,
        })
        .map_err(|_| ValidationError)?;
        validation::ensure(bytes.len() <= MAX_REQUEST_BYTES)?;
        let _guard = CancelOnDrop(cancel.clone());
        if cancel.is_cancelled() {
            return Err(ClientError::Transport(TransportError::Cancelled));
        }
        let limits = RequestLimits {
            deadline: Instant::now() + Duration::from_secs(30),
            cancel: cancel.clone(),
            max_request_bytes: MAX_REQUEST_BYTES,
            max_response_bytes: MAX_RESPONSE_BYTES,
        };
        let bytes = self
            .transport
            .send(bytes, limits.clone())
            .await
            .map_err(ClientError::Transport)?;
        if cancel.is_cancelled() {
            return Err(ClientError::Transport(TransportError::Cancelled));
        }
        if Instant::now() >= limits.deadline {
            return Err(ClientError::Transport(TransportError::Deadline));
        }
        Ok(decode(&bytes)?)
    }
}
struct CancelOnDrop(CancellationToken);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
