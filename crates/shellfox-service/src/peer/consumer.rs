//! Shared outbound consumer logic for the native Desktop and headless CLI.
//! Transport is a validated local Service bridge; this module opens no socket.
use anyhow::{Result, anyhow, ensure};
use shellfox_client::manager_v2::{
    DiscoveryOutcome, FallbackPolicy, ManagerBindingWitness, ManagerClient, Status,
};
use shellfox_protocol::{OutboundTransport, manager_v2 as m};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[cfg(test)]
#[path="native_peer_intent_tests.rs"]mod intent_tests;
#[derive(Clone)]
pub struct Origin {
    pub cycle: u64,
    pub witness: ManagerBindingWitness,
}
#[derive(Default)]
struct Negotiation {
    cycle: u64,
    discovered: bool,
    attempt: Option<m::Id>,
}
#[derive(Debug)]
pub struct LegacyPeerRequired;
impl std::fmt::Display for LegacyPeerRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("This peer offers only the legacy Shellfox profile.")
    }
}
impl std::error::Error for LegacyPeerRequired {}

pub struct PeerConnection<T: OutboundTransport> {
    client: Arc<ManagerClient<T>>,
    client_id: m::Id,
    cycle: AtomicU64,
    negotiation: tokio::sync::Mutex<Negotiation>,
}
impl<T: OutboundTransport> PeerConnection<T> {
    pub fn new(client: Arc<ManagerClient<T>>, client_id: m::Id) -> Self {
        Self {
            client,
            client_id,
            cycle: AtomicU64::new(1),
            negotiation: tokio::sync::Mutex::new(Negotiation::default()),
        }
    }
    pub fn client(&self) -> Arc<ManagerClient<T>> {
        self.client.clone()
    }
    pub fn status(&self) -> Status {
        self.client.status()
    }
    pub fn origin(&self) -> Result<Origin> {
        let witness = self.client.binding_witness().map_err(|e| anyhow!(e))?;
        witness
            .commit_if_current(|| Origin {
                cycle: self.cycle.load(Ordering::Acquire),
                witness: witness.clone(),
            })
            .map_err(|e| anyhow!(e))
    }
    pub fn is_current(&self, origin: &Origin) -> bool {
        origin.cycle == self.cycle.load(Ordering::Acquire)
            && self
                .client
                .binding_witness()
                .is_ok_and(|current| current.same_origin(&origin.witness))
    }
    /// One PAIR per peer connection, shared by all bound views. No view calls this
    /// as an implicit takeover. A legacy offer is not a downgrade/authority grant.
    pub async fn connect(&self) -> Result<Origin> {
        self.connect_with_policy(FallbackPolicy::Never).await
    }
    pub async fn connect_with_policy(&self, fallback: FallbackPolicy) -> Result<Origin> {
        let mut negotiation = self.negotiation.lock().await;
        if self.client.binding_witness().is_ok() {
            return self.origin();
        }
        let cycle = self.cycle.load(Ordering::Acquire);
        if negotiation.cycle != cycle {
            *negotiation = Negotiation {
                cycle,
                ..Negotiation::default()
            };
        }
        if !negotiation.discovered {
            match self
                .client
                .discover(fallback)
                .await
                .map_err(|e| anyhow!(e))?
            {
                DiscoveryOutcome::Available(_) => negotiation.discovered = true,
                DiscoveryOutcome::LegacyAvailable => return Err(LegacyPeerRequired.into()),
                DiscoveryOutcome::Incompatible(_) => {
                    return Err(anyhow!(
                        "This host does not offer the required Manager profile. No fallback or control was attempted."
                    ));
                }
            }
        }
        ensure!(
            cycle == self.cycle.load(Ordering::Acquire),
            "Peer connection was retired during discovery."
        );
        // Approval continuation and an uncertain PAIR reuse THIS exact attempt.
        // Only explicit reconnect/retirement creates another attempt or owner offer.
        let attempt = *negotiation.attempt.get_or_insert_with(m::Id::new);
        reply(self.client.pair(attempt).await.map_err(|e| anyhow!(e))?)?;
        let origin = self.origin()?;
        ensure!(
            origin.cycle == cycle,
            "Peer connection was retired during pairing."
        );
        Ok(origin)
    }
    pub async fn execute<R: m::Request>(
        &self,
        origin: &Origin,
        request: &R,
    ) -> Result<R::Response> {
        ensure!(
            self.is_current(origin),
            "Remote operation belongs to a retired peer connection."
        );
        let value = reply(
            self.client
                .execute_with_witness(&origin.witness, request)
                .await
                .map_err(|e| anyhow!(e))?,
        )?;
        ensure!(
            self.is_current(origin),
            "Remote response belongs to a retired peer connection."
        );
        Ok(value)
    }
    pub async fn snapshot(&self, origin: &Origin) -> Result<m::Snapshot> {
        self.execute(origin, &m::GetSnapshot(shellfox_protocol::GetSnapshot {}))
            .await
    }
    /// Local retirement invalidates every source/selection/save/view token before
    /// a new discovery. It does not close any host terminal or replay input.
    pub fn reconnect(&self) -> Result<()> {
        ensure!(
            self.cycle.load(Ordering::Acquire) != u64::MAX,
            "Peer connection cycle exhausted."
        );
        // The actual client mutex/cycle is authority. Retire it before advancing
        // the secondary UI cancellation counter, never mint origin from that counter.
        self.client.reconnect(self.client_id);
        self.cycle
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| anyhow!("Peer connection cycle exhausted."))?;
        Ok(())
    }
    pub async fn disconnect(&self) -> Result<()> {
        self.cycle
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                value.checked_add(1)
            })
            .map_err(|_| anyhow!("Peer connection cycle exhausted."))?;
        match self.client.disconnect().await {
            Ok(value) => {
                reply(value)?;
                Ok(())
            }
            Err(shellfox_client::manager_v2::ClientError::NotConnected) => Ok(()),
            Err(error) => Err(anyhow!(error)),
        }
    }
}
/// Frontend request intent only. Unknown metadata may request cold IfFree;
/// only known non-PTY kinds choose delivery without control. The backend still
/// decides source availability, source policy and every actual grant.
pub fn attach_control(snapshot: &m::Snapshot, tab: m::Id) -> Result<m::ControlRequest> {
    let kind = snapshot
        .entries
        .iter()
        .find_map(|entry| match entry {
            m::CatalogEntry::Session { value } => value
                .tabs
                .iter()
                .find(|value| value.id == tab)
                .map(|value| &value.source_kind),
            _ => None,
        })
        .ok_or_else(|| anyhow!("Tab is not present in this peer's catalog."))?;
    Ok(match kind.known() {
        Some(m::SourceKind::AutomationPipe | m::SourceKind::ExternalBytes) => {
            m::ControlRequest::None
        }
        _ => m::ControlRequest::IfFree,
    })
}
/// Freeze the catalog fact at native-event admission; outer None means genuinely
/// Unavailable, Some(None) is an actually captured absence, never interchangeable.
pub fn settlement(snapshot: &m::Snapshot, tab: m::Id) -> Option<Option<String>> {
    let session = snapshot.entries.iter().find_map(|entry| match entry {
        m::CatalogEntry::Session { value } if value.tabs.iter().any(|item| item.id == tab) => {
            Some(value)
        }
        _ => None,
    })?;
    session.facts.settled_at.known().cloned()
}
pub fn reply<T>(reply: m::Reply<T>) -> Result<T> {
    match reply {
        m::Reply::Success(value) => Ok(value),
        m::Reply::Failure(error) => Err(anyhow!("Remote {:?}: {}", error.code, error.message)),
    }
}
