//! Per-window registry of V1 peer bindings. Each peer has at most one binding,
//! shared by its catalog, terminals and documents.
use super::peer_v1::V1Binding;
use anyhow::{Result, anyhow, ensure};
use shellfox_protocol::AuthenticatedPeerId;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};

#[derive(Default)]
pub struct Peers {
    v1: Mutex<HashMap<AuthenticatedPeerId, Arc<V1Binding>>>,
}
impl Peers {
    fn bindings(&self) -> Result<MutexGuard<'_, HashMap<AuthenticatedPeerId, Arc<V1Binding>>>> {
        self.v1
            .lock()
            .map_err(|_| anyhow!("Peer connection registry poisoned."))
    }
    pub async fn disconnect(&self, peer: &AuthenticatedPeerId) -> Result<()> {
        let binding = self.bindings()?.get(peer).cloned();
        if let Some(binding) = binding {
            binding.disconnect().await?;
            self.bindings()?.remove(peer);
        }
        Ok(())
    }
    pub async fn close_window(&self) -> Result<()> {
        let bindings: Vec<_> = self.bindings()?.values().cloned().collect();
        let mut failures = Vec::new();
        for binding in bindings {
            if let Err(error) = binding.disconnect().await {
                failures.push(error.to_string());
            }
        }
        ensure!(
            failures.is_empty(),
            "Peer view withdrawal needs retry: {}",
            failures.join("; ")
        );
        Ok(())
    }
}
impl super::HostClient {
    pub fn shared_peer_v1(&self, peer: &AuthenticatedPeerId) -> Result<Arc<V1Binding>> {
        let mut bindings = self.peers.bindings()?;
        if let Some(binding) = bindings.get(peer) {
            return Ok(binding.clone());
        }
        ensure!(
            bindings.len() < 64,
            "This window has too many peer connections."
        );
        let binding = Arc::new(self.peer_v1(peer.clone()));
        bindings.insert(peer.clone(), binding.clone());
        Ok(binding)
    }

    pub(crate) fn retry_rejected_v1(&self, peer: &AuthenticatedPeerId) {
        if let Ok(bindings) = self.peers.bindings()
            && let Some(binding) = bindings.get(peer)
        {
            binding.retry_rejected();
        }
    }

    pub async fn disconnect_peer(&self, peer: &AuthenticatedPeerId) -> Result<()> {
        self.peers.disconnect(peer).await
    }
}
