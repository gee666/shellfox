use super::*;
use crate::host_client::{
    peer::{Connection, SourceAdmission},
    peer_v1::V1Binding,
};
use shellfox_protocol as wire;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) enum PeerApi {
    Legacy(Arc<V1Binding>),
    Manager(Arc<Connection>, SourceAdmission),
}

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
    pub(crate) async fn snapshot(&self) -> anyhow::Result<m::Snapshot> {
        match self {
            Self::Legacy(binding) => catalog_v1::project(binding.v1_snapshot().await?),
            Self::Manager(connection, admission) => {
                connection
                    .execute_admitted(admission, &m::GetSnapshot(wire::GetSnapshot {}))
                    .await
            }
        }
    }

    /// The two requests describe the same operation. Outside the selected
    /// branch no request is sent, including on denial or unknown outcomes.
    pub(crate) async fn call<L: wire::Request, M: m::Request, T>(
        &self,
        legacy: L,
        manager: M,
        project_legacy: impl FnOnce(L::Response) -> T,
        project_manager: impl FnOnce(M::Response) -> T,
    ) -> Result<T, PeerCallError> {
        match self {
            Self::Legacy(binding) => match binding.v1_reply(&legacy).await? {
                wire::Reply::Success(value) => Ok(project_legacy(value)),
                wire::Reply::Failure(error) => Err(PeerCallError {
                    message: error.message,
                    code: Some(error.code),
                }),
            },
            Self::Manager(connection, admission) => match connection
                .execute_admitted_reply(admission, &manager)
                .await?
            {
                m::Reply::Success(value) => Ok(project_manager(value)),
                m::Reply::Failure(error) => Err(PeerCallError {
                    message: error.message,
                    code: Some(error.code),
                }),
            },
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
        if let Some(binding) = self.legacy_peer_binding(&cycle.peer) {
            return Ok(PeerApi::Legacy(binding));
        }
        let witness = state
            .origin
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Peer catalog origin is unavailable."))?;
        let connection = self
            .host
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Host is unavailable."))?
            .peer_existing(&cycle.peer)?;
        let admission = connection.admit_source_with(witness)?;
        Ok(PeerApi::Manager(connection, admission))
    }
}
