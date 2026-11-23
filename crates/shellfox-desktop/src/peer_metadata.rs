use super::*;
use crate::host_client::peer_v1::V1Binding;
use shellfox_protocol as wire;
use std::sync::Arc;

/// Requests against one connected peer, through its V1 binding.
#[derive(Clone)]
pub(crate) struct PeerApi(Arc<V1Binding>);

pub(crate) struct PeerCallError {
    pub message: String,
    pub code: Option<wire::ErrorCode>,
}
impl From<anyhow::Error> for PeerCallError {
    fn from(error: anyhow::Error) -> Self {
        Self {
            message: error.to_string(),
            code: None,
        }
    }
}

impl PeerApi {
    pub(crate) fn binding(&self) -> &Arc<V1Binding> {
        &self.0
    }

    pub(crate) async fn snapshot(&self) -> anyhow::Result<wire::ManagerSnapshot> {
        self.0.v1_snapshot().await
    }

    pub(crate) async fn call<R: wire::Request, T>(
        &self,
        request: R,
        project: impl FnOnce(R::Response) -> T,
    ) -> Result<T, PeerCallError> {
        match self.0.v1_reply(&request).await? {
            wire::Reply::Success(value) => Ok(project(value)),
            wire::Reply::Failure(error) => Err(PeerCallError {
                message: error.message,
                code: Some(error.code),
            }),
        }
    }
}

impl Root {
    pub(crate) fn legacy_peer_binding(&self, peer: &AuthenticatedPeerId) -> Option<Arc<V1Binding>> {
        self.peer_ui.legacy.get(peer).cloned()
    }

    pub(crate) fn peer_api(&self, cycle: &source::PeerCycle) -> anyhow::Result<PeerApi> {
        anyhow::ensure!(
            !self.peer_ui.frozen && self.peers.current(cycle),
            "Peer binding is no longer current."
        );
        let state = &self.peers.peers[&cycle.peer];
        anyhow::ensure!(
            state.phase == source::Phase::Connected,
            "Peer is not connected."
        );
        self.legacy_peer_binding(&cycle.peer)
            .map(PeerApi)
            .ok_or_else(|| anyhow::anyhow!("Peer is not connected."))
    }
}
