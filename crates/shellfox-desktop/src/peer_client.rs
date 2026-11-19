//! Per-window peer registry. NativeClient is the validated Service transport,
//! never a renderer-owned socket, helper or Tailscale node.
#[path = "../../native_peer_consumer.rs"]
mod connection;
pub use connection::{LegacyPeerRequired, PeerConnection};
#[cfg(feature = "experimental-shellfox-native")]
pub use connection::attach_control;
#[cfg(any(test, feature = "experimental-shellfox-native"))]
pub use connection::settlement;
#[path = "peer_admission.rs"]
mod admission;
#[cfg(test)]
#[path = "peer_client_tests.rs"]
mod tests;
pub(crate) use admission::SourceAdmission;
#[path = "../../native_peer_transport.rs"]
mod transport;
pub type Transport = transport::ServiceTransport<NativeClient>;
use anyhow::{Result, anyhow, ensure};
use shellfox_protocol::AuthenticatedPeerId;
use shellfox_service::native::NativeClient;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub type Connection = PeerConnection<Transport>;
#[derive(Default)]
pub struct Peers {
    manager: Mutex<HashMap<AuthenticatedPeerId, Arc<Connection>>>,
    v1: Mutex<HashMap<AuthenticatedPeerId, Arc<super::peer_v1::V1Binding>>>,
}
impl Peers {
    pub fn get(&self, native: &NativeClient, peer: AuthenticatedPeerId) -> Result<Arc<Connection>> {
        let mut peers = self
            .manager
            .lock()
            .map_err(|_| anyhow!("Peer connection registry poisoned."))?;
        if let Some(client) = peers.get(&peer) {
            return Ok(client.clone());
        }
        ensure!(
            peers.len() < 64,
            "This window has too many peer connections."
        );
        let id = shellfox_protocol::manager_v2::Id::new();
        let transport = transport::ServiceTransport::new(native.clone()).with_legacy_discovery();
        #[cfg(feature = "experimental-shellfox-native")]
        let manager = shellfox_mobile::ManagerRecovery::native_client(
            peer.clone(),
            id,
            transport,
        )
        .map_err(|e| anyhow!("Native Manager client unavailable: {e:?}"))?;
        #[cfg(not(feature = "experimental-shellfox-native"))]
        let manager = Arc::new(shellfox_client::manager_v2::ManagerClient::new(
            peer.clone(), id, transport,
        ));
        let connection = Arc::new(PeerConnection::new(manager, id));
        peers.insert(peer, connection.clone());
        Ok(connection)
    }
    pub fn existing(&self, peer: &AuthenticatedPeerId) -> Result<Arc<Connection>> {
        self.manager
            .lock()
            .map_err(|_| anyhow!("Peer connection registry poisoned."))?
            .get(peer)
            .cloned()
            .ok_or_else(|| anyhow!("Peer connection is unavailable."))
    }
    pub async fn disconnect(&self, peer: &AuthenticatedPeerId) -> Result<()> {
        let v1 = self.v1.lock().map_err(|_| anyhow!("Peer connection registry poisoned."))?
            .get(peer).cloned();
        if let Some(binding) = &v1 {
            binding.disconnect().await?;
            self.v1.lock().map_err(|_| anyhow!("Peer connection registry poisoned."))?.remove(peer);
        }
        match self.existing(peer) {
            Ok(binding) => binding.disconnect().await,
            Err(_) if v1.is_some() => Ok(()),
            Err(error) => Err(error),
        }
    }
    pub async fn close_window(&self) -> Result<()> {
        let clients: Vec<_> = self
            .manager
            .lock()
            .map_err(|_| anyhow!("Peer connection registry poisoned."))?
            .values()
            .cloned()
            .collect();
        let v1: Vec<_> = self.v1.lock()
            .map_err(|_| anyhow!("Peer connection registry poisoned."))?
            .values().cloned().collect();
        let mut failures = Vec::new();
        for binding in v1 {
            if let Err(error) = binding.disconnect().await {
                failures.push(error.to_string());
            }
        }
        for client in clients {
            if let Err(error) = client.disconnect().await {
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
    pub fn shared_peer_v1(&self, peer: &AuthenticatedPeerId) -> Result<Arc<super::peer_v1::V1Binding>> {
        let mut clients = self.peers.v1.lock()
            .map_err(|_| anyhow!("Peer connection registry poisoned."))?;
        if let Some(binding) = clients.get(peer) { return Ok(binding.clone()); }
        ensure!(clients.len() < 64, "This window has too many peer connections.");
        let binding = Arc::new(self.peer_v1(peer.clone()));
        clients.insert(peer.clone(), binding.clone());
        Ok(binding)
    }

    pub(crate) fn retry_rejected_v1(&self, peer: &AuthenticatedPeerId) {
        if let Ok(clients) = self.peers.v1.lock() {
            if let Some(binding) = clients.get(peer) {
                binding.retry_rejected();
            }
        }
    }

    pub fn peer_connection(&self, peer: AuthenticatedPeerId) -> Result<Arc<Connection>> {
        self.peers.get(self.native()?, peer)
    }
    pub fn peer_existing(&self, peer: &AuthenticatedPeerId) -> Result<Arc<Connection>> {
        self.peers.existing(peer)
    }
    pub async fn disconnect_peer(&self, peer: &AuthenticatedPeerId) -> Result<()> {
        self.peers.disconnect(peer).await
    }
}
