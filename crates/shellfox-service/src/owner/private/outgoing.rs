use super::*;
use crate::remote::{outgoing, outgoing_wire, PeerResponse};
use shellfox_protocol::Endpoint;
use shellfox_protocol::{AuthenticatedPeerId, CancellationToken, RequestLimits, TransportError};
impl PrivateOwner {
    pub(super) fn cancel_outgoing_fences(&self) {
        for lifetime in self.lifetimes.values() {
            lifetime.cancel.cancel();
        }
    }
    pub(super) fn caller_fence(
        &self,
        service: &Service,
        client_id: uuid::Uuid,
        epoch: uuid::Uuid,
    ) -> Result<CancellationToken> {
        service.assert_accepting()?;
        anyhow::ensure!(!client_id.is_nil(), "Outgoing caller identity unavailable");
        let lifetime = self
            .lifetimes
            .get(&client_id)
            .filter(|l| l.epoch == epoch)
            .context("Original private client lifetime is stale or closed")?;
        Ok(lifetime.cancel.clone())
    }
}
impl Drop for PrivateOwner {
    fn drop(&mut self) {
        self.cancel_outgoing_fences();
    }
}
impl OwnerClient {
    pub(crate) async fn peer_request_with_status(
        &self,
        peer: &AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
        epoch: uuid::Uuid,
    ) -> std::result::Result<PeerResponse, TransportError> {
        let limits = outgoing::validate(&body, limits)?;
        let remote = self.remote.as_ref().ok_or(TransportError::Unavailable)?;
        let (reply, response) = oneshot::channel();
        self.submit_private(PrivateTask::OutgoingFence {
            client_id: self.client_id,
            epoch,
            reply,
        })
        .await
        .map_err(|_| TransportError::Unavailable)?;
        let original = tokio::select! {
            biased;
            _=limits.cancel.cancelled()=>return Err(TransportError::Cancelled),
            _=remote.outgoing_deadline(limits.deadline)=>return Err(TransportError::Deadline),
            result=response=>result.map_err(|_|TransportError::Unavailable)?.map_err(|_|TransportError::Unavailable)?,
        };
        tokio::select! {
            biased;
            _=original.cancelled()=>Err(TransportError::Unavailable),
            result=remote.peer_request(peer,endpoint,body,limits)=>result,
        }
    }
    pub(crate) async fn private_outgoing(
        &self,
        request: outgoing_wire::Request,
        epoch: uuid::Uuid,
    ) -> Result<serde_json::Value> {
        let (peer, endpoint, body, limits) = request
            .into_parts(CancellationToken::new())
            .map_err(|_| anyhow::anyhow!("Invalid private outgoing request"))?;
        let maximum = limits.max_response_bytes;
        let result = self
            .peer_request_with_status(&peer, endpoint, body, limits, epoch)
            .await;
        let reply: outgoing_wire::Reply = result
            .and_then(|r| outgoing_wire::Response::from_http(r, maximum))
            .map_err(outgoing_wire::Failure::from);
        Ok(serde_json::to_value(reply)?)
    }
}
