//! Per-window peer registry. NativeClient is the validated Service transport,
//! never a renderer-owned socket, helper or Tailscale node.
#[path = "../../native_peer_consumer.rs"]
mod connection;
pub use connection::{PeerConnection, attach_control, settlement};
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
pub struct Peers(Mutex<HashMap<AuthenticatedPeerId, Arc<Connection>>>);
impl Peers {
    pub fn get(&self, native: &NativeClient, peer: AuthenticatedPeerId) -> Result<Arc<Connection>> {
        let mut peers = self
            .0
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
        let manager = shellfox_mobile::ManagerRecovery::native_client(
            peer.clone(),
            id,
            transport::ServiceTransport::new(native.clone()),
        )
        .map_err(|e| anyhow!("Native Manager client unavailable: {e:?}"))?;
        let connection = Arc::new(PeerConnection::new(manager, id));
        peers.insert(peer, connection.clone());
        Ok(connection)
    }
    pub fn existing(&self, peer: &AuthenticatedPeerId) -> Result<Arc<Connection>> {
        self.0
            .lock()
            .map_err(|_| anyhow!("Peer connection registry poisoned."))?
            .get(peer)
            .cloned()
            .ok_or_else(|| anyhow!("Peer connection is unavailable."))
    }
    pub async fn disconnect(&self, peer: &AuthenticatedPeerId) -> Result<()> {
        self.existing(peer)?.disconnect().await
    }
    pub async fn close_window(&self) -> Result<()> {
        let clients: Vec<_> = self
            .0
            .lock()
            .map_err(|_| anyhow!("Peer connection registry poisoned."))?
            .values()
            .cloned()
            .collect();
        let mut failures = Vec::new();
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
    pub fn peer_connection(&self, peer: AuthenticatedPeerId) -> Result<Arc<Connection>> {
        self.peers.get(&self.native, peer)
    }
    pub fn peer_existing(&self, peer: &AuthenticatedPeerId) -> Result<Arc<Connection>> {
        self.peers.existing(peer)
    }
    pub async fn disconnect_peer(&self, peer: &AuthenticatedPeerId) -> Result<()> {
        self.peers.disconnect(peer).await
    }
}
