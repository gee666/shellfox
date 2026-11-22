//! Non-GPUI client of the central service. UI code awaits these futures; it never
//! opens Host/SQLite or consumes runtime events directly.
#[path = "host_client_attached.rs"]
pub(crate) mod attached;
#[path = "host_client_catalog.rs"]
mod catalog;
#[path = "host_client_changes.rs"]
mod changes;
#[path = "peer_v1.rs"]
pub mod peer_v1;
#[path = "peer_client.rs"] pub mod peer;
#[path = "host_client_endpoint.rs"]
mod endpoint;
#[path = "host_client_frames.rs"]
mod frames;
#[path = "host_client_presentation.rs"]
mod presentation;
#[path = "host_client_launch.rs"]
mod launch;
pub use launch::handoff_existing;
#[cfg(test)]
#[path = "host_client_tests.rs"]
mod tests;

use anyhow::{Result, ensure};
pub use endpoint::{HostEndpoint, HostEndpointConfig, HostEndpointStartupOutcome};
use shellfox_core::TabId;
use shellfox_service::{
    Attachment,
    native::{
        NativeAction, NativeAdmissionError, NativeClient, NativeCommand, NativeFrame,
        NativeInputContext, NativeObserverAttachment, NativePresentation, NativeResponse,
        NativeStatus, NativeViewAttachment, NativeWatches,
    },
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

const MAX_REQUESTS: usize = 16;
const MAX_FRAME_REQUESTS: usize = 2;
const MAX_FRAME_WAITERS: usize = 128;
#[derive(Clone)]
enum HostPort {
    Native(NativeClient),
    Attached(attached::AttachedLocal),
}
#[derive(Clone)]
pub struct HostClient {
    port: HostPort,
    peers: Arc<peer::Peers>,
    admission: Arc<AtomicUsize>,
    frame_admission: Arc<AtomicUsize>,
    frame_slots: Arc<tokio::sync::Semaphore>,
    frames: Arc<Mutex<frames::Frames>>,
    watches: Arc<tokio::sync::OnceCell<NativeWatches>>,
}
struct Admission(Arc<AtomicUsize>);
impl Admission {
    fn acquire(counter: Arc<AtomicUsize>) -> Result<Self> {
        Self::acquire_limit(counter, MAX_REQUESTS)
    }
    fn acquire_limit(counter: Arc<AtomicUsize>, limit: usize) -> Result<Self> {
        counter
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                (current < limit).then_some(current + 1)
            })
            .map_err(|_| {
                anyhow::anyhow!("Native client request limit reached; nothing was enqueued.")
            })?;
        Ok(Self(counter))
    }
}
impl Drop for Admission {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
impl HostClient {
    pub fn new(native: NativeClient) -> Self {
        Self::with_port(HostPort::Native(native))
    }
    pub(crate) fn attached(local: attached::AttachedLocal) -> Self {
        Self::with_port(HostPort::Attached(local))
    }
    fn with_port(port: HostPort) -> Self {
        Self {
            port,
            peers: Arc::new(peer::Peers::default()),
            admission: Arc::new(AtomicUsize::new(0)),
            frame_admission: Arc::new(AtomicUsize::new(0)),
            frame_slots: Arc::new(tokio::sync::Semaphore::new(MAX_FRAME_REQUESTS)),
            frames: Arc::new(Mutex::new(frames::Frames::default())),
            watches: Arc::default(),
        }
    }
    /// Clone shares a window/controller. Fork gives a new window a private UUID,
    /// latest-frame cache and admission budget, with the SAME service owner.
    pub fn fork(&self) -> Self {
        match &self.port {
            HostPort::Native(client) => Self::new(client.fork()),
            HostPort::Attached(_) => self.clone(),
        }
    }
    pub(crate) fn connected_to_headless(&self) -> bool {
        matches!(self.port, HostPort::Attached(_))
    }

    fn native(&self) -> Result<&NativeClient> {
        match &self.port {
            HostPort::Native(client) => Ok(client),
            HostPort::Attached(_) => anyhow::bail!("This capability is not available on an attached desktop."),
        }
    }
    fn local(&self) -> Option<&attached::AttachedLocal> {
        match &self.port { HostPort::Attached(client) => Some(client), _ => None }
    }
    fn admit(&self) -> Result<Admission> {
        Admission::acquire(self.admission.clone())
    }
    pub fn same_window_client(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.admission, &other.admission)
    }
    pub(crate) async fn source_facts_watch(
        &self,
    ) -> Result<shellfox_service::activity_integration::SourceFactsWatch> {
        let _admission = self.admit()?;
        self.native()?.source_facts_watch().await
    }

    /// In-process change signals. None on an attached desktop: its host parks
    /// the same requests until something changes.
    pub(crate) async fn watches(&self) -> Option<&NativeWatches> {
        let HostPort::Native(native) = &self.port else {
            return None;
        };
        self.watches.get_or_try_init(|| native.watches()).await.ok()
    }
    pub async fn power_status(&self) -> Result<shellfox_service::power::PowerReport> {
        let _admission = self.admit()?;
        if let Some(client) = self.local() {
            client.request(shellfox_service::power::PowerRequest::Status {}).await
        } else {
            self.native()?.power_status().await
        }
    }
    pub async fn set_power(&self, active: bool) -> Result<shellfox_service::power::PowerReport> {
        let _admission = self.admit()?;
        if let Some(client) = self.local() {
            client.request(shellfox_service::power::PowerRequest::Set { active }).await
        } else {
            self.native()?.set_power(active).await
        }
    }
    pub async fn creation_request_receipt(
        &self,
        request_id: shellfox_core::SessionId,
    ) -> Result<shellfox_host::CreationRequestReceipt> {
        match self.execute(NativeAction::CreationRequestReceipt { request_id }).await? {
            NativeResponse::CreationRequestReceipt { request_id: id, receipt } if id == request_id => Ok(receipt),
            _ => anyhow::bail!("Unexpected creation receipt."),
        }
    }
    pub async fn execute(&self, action: NativeAction) -> Result<NativeResponse> {
        action
            .validate()
            .map_err(|error| NativeAdmissionError::Invalid(error.to_string()))?;
        let _admission = match self.admit() {
            Ok(admission) => admission,
            Err(_) => return Err(NativeAdmissionError::Full(Box::new(action)).into()),
        };
        let detached = match &action {
            NativeAction::Detach { tab, .. } => Some((*tab, false)),
            NativeAction::Unobserve { tab, .. } => Some((*tab, true)),
            _ => None,
        };
        let detach_all = matches!(action, NativeAction::DetachAll);
        let response = match &self.port {
            HostPort::Native(client) => client.execute(action).await?,
            HostPort::Attached(client) => match action {
                NativeAction::Remote(request) => NativeResponse::Remote(client.remote(request).await?),
                action @ (NativeAction::System(_) | NativeAction::Ssh(_)) => client.provider_request(action).await?,
                action => NativeResponse::from(client.request(shellfox_service::local::DesktopRequest::try_from(action)?).await?),
            },
        };
        if detached.is_some() || detach_all || matches!(response, NativeResponse::Snapshot(_)) {
            let mut frames = self
                .frames
                .lock()
                .map_err(|_| anyhow::anyhow!("Native frame cache poisoned."))?;
            if let Some((tab, observer)) = detached {
                frames.remove(tab, observer);
            }
            if detach_all {
                frames.clear();
            }
            if let NativeResponse::Snapshot(snapshot) = &response {
                frames.retain_snapshot(snapshot);
            }
        }
        Ok(response)
    }
    pub async fn status(&self) -> Result<NativeStatus> {
        match self.execute(NativeAction::Status).await? {
            NativeResponse::Status(status) => Ok(status),
            _ => anyhow::bail!("Service returned an invalid native status response."),
        }
    }
    pub async fn attach(&self, tab: TabId, takeover: bool) -> Result<Attachment> {
        match self.execute(NativeAction::Attach { tab, takeover }).await? {
            NativeResponse::Attached(attachment) => Ok(attachment),
            _ => anyhow::bail!("Service returned an invalid native attachment."),
        }
    }
    pub async fn command(
        &self,
        tab: TabId,
        attachment: Attachment,
        command: NativeCommand,
        input: Option<NativeInputContext>,
    ) -> Result<NativeResponse> {
        self.execute(NativeAction::Terminal {
            tab,
            attachment,
            command,
            input,
        })
        .await
    }
    pub async fn detach(&self, tab: TabId, attachment: Attachment) -> Result<()> {
        self.execute(NativeAction::Detach { tab, attachment })
            .await?;
        self.frames
            .lock()
            .map_err(|_| anyhow::anyhow!("Native frame cache poisoned."))?
            .remove(tab, false);
        Ok(())
    }
    pub(crate) async fn close_window_client(&self) -> Result<()> {
        if let Some(client) = self.local() {
            self.peers.close_window().await?;
            // Disconnect also drops the verified lifetime socket on errors. The
            // owner, if alive, releases this epoch; never stop or quit that host.
            if let Err(error) = client.disconnect().await {
                eprintln!("Attached desktop disconnected: {error:#}");
            }
            return Ok(());
        }
        self.detach_all().await
    }

    pub async fn detach_all(&self) -> Result<()> {
        let local = self.execute(NativeAction::DetachAll).await;
        let peer = self.peers.close_window().await;
        local?;
        peer?;
        self.frames
            .lock()
            .map_err(|_| anyhow::anyhow!("Native frame cache poisoned."))?
            .clear();
        Ok(())
    }
}
