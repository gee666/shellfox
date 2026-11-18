//! Non-GPUI client of the central service. UI code awaits these futures; it never
//! opens Host/SQLite or consumes runtime events directly.
#[path = "host_client_catalog.rs"]
mod catalog;
#[path = "peer_v1.rs"]
pub mod peer_v1;
#[cfg(feature = "experimental-shellfox-native")]
#[path = "peer_client.rs"] pub mod peer;
#[path = "host_client_endpoint.rs"]
mod endpoint;
#[path = "host_client_frames.rs"]
mod frames;
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
        NativeStatus, NativeViewAttachment,
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
pub struct HostClient {
    native: NativeClient,
    #[cfg(feature = "experimental-shellfox-native")]
    peers: Arc<peer::Peers>,
    admission: Arc<AtomicUsize>,
    frame_admission: Arc<AtomicUsize>,
    frame_slots: Arc<tokio::sync::Semaphore>,
    frames: Arc<Mutex<frames::Frames>>,
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
        Self {
            native,
            #[cfg(feature = "experimental-shellfox-native")]
            peers: Arc::new(peer::Peers::default()),
            admission: Arc::new(AtomicUsize::new(0)),
            frame_admission: Arc::new(AtomicUsize::new(0)),
            frame_slots: Arc::new(tokio::sync::Semaphore::new(MAX_FRAME_REQUESTS)),
            frames: Arc::new(Mutex::new(frames::Frames::default())),
        }
    }
    /// Clone shares a window/controller. Fork gives a new window a private UUID,
    /// latest-frame cache and admission budget, with the SAME service owner.
    pub fn fork(&self) -> Self {
        Self::new(self.native.fork())
    }
    fn admit(&self) -> Result<Admission> {
        Admission::acquire(self.admission.clone())
    }
    pub fn same_window_client(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.admission, &other.admission)
    }
    pub async fn power_status(&self) -> Result<shellfox_service::power::PowerReport> {
        let _admission = self.admit()?;
        self.native.power_status().await
    }
    pub async fn set_power(&self, active: bool) -> Result<shellfox_service::power::PowerReport> {
        let _admission = self.admit()?;
        self.native.set_power(active).await
    }
    pub async fn creation_request_receipt(
        &self,
        request_id: shellfox_core::SessionId,
    ) -> Result<shellfox_host::CreationRequestReceipt> {
        let _admission = self.admit()?;
        self.native.creation_request_receipt(request_id).await
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
        let response = self.native.execute(action).await?;
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
    pub async fn frame(&self, tab: TabId, attachment: Attachment) -> Result<Option<NativeFrame>> {
        self.frame_focused(tab, attachment, true).await
    }
    pub async fn frame_focused(
        &self,
        tab: TabId,
        attachment: Attachment,
        focused: bool,
    ) -> Result<Option<NativeFrame>> {
        // A bounded fair queue avoids starving later hidden tabs, while only
        // two frame cycles compete with ordered key/release admission.
        NativeAction::Presentation {
            tab,
            attachment: NativeViewAttachment::Controller(attachment.clone()),
            action: NativePresentation::Frame { focused },
        }
        .validate()
        .map_err(|error| NativeAdmissionError::Invalid(error.to_string()))?;
        let _frame_admission =
            match Admission::acquire_limit(self.frame_admission.clone(), MAX_FRAME_WAITERS) {
                Ok(slot) => slot,
                Err(_) => {
                    return Err(NativeAdmissionError::Full(Box::new(NativeAction::Frame {
                        tab,
                        attachment,
                    }))
                    .into());
                }
            };
        let _permit = self
            .frame_slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| {
                NativeAdmissionError::Stopped(Box::new(NativeAction::Frame {
                    tab,
                    attachment: attachment.clone(),
                }))
            })?;
        let NativeResponse::Frame(frame) = self
            .execute(NativeAction::Presentation {
                tab,
                attachment: NativeViewAttachment::Controller(attachment.clone()),
                action: NativePresentation::Frame { focused },
            })
            .await?
        else {
            anyhow::bail!("Service returned an invalid native frame response.");
        };
        let Some(frame) = frame else {
            return Ok(None);
        };
        ensure!(
            frame.frame.identity.tab_id == tab
                && frame.frame.identity.generation == attachment.generation
                && matches!(frame.presentation.view, shellfox_runtime::PresentationLease::Controller(lease) if lease.controller == attachment.controller && lease.epoch == attachment.epoch),
            "Native frame identity/view mismatch."
        );
        self.frames
            .lock()
            .map_err(|_| anyhow::anyhow!("Native frame cache poisoned."))?
            .accept(tab, false, frame.clone())?;
        // Bounded native state has accepted the immutable frame. Painting is not
        // the consumption boundary, and receive/enqueue alone never sends ACKs.
        if !frame.replay_lost {
            self.execute(NativeAction::AcceptFrame {
                tab,
                attachment,
                frame_id: frame.frame_id,
            })
            .await?;
        }
        Ok(Some(frame))
    }
    pub async fn observer_frame(
        &self,
        tab: TabId,
        attachment: NativeObserverAttachment,
    ) -> Result<Option<NativeFrame>> {
        self.observer_frame_focused(tab, attachment, true).await
    }
    pub async fn observer_frame_focused(
        &self,
        tab: TabId,
        attachment: NativeObserverAttachment,
        focused: bool,
    ) -> Result<Option<NativeFrame>> {
        NativeAction::ObserverFrame {
            tab,
            attachment: attachment.clone(),
        }
        .validate()
        .map_err(|error| NativeAdmissionError::Invalid(error.to_string()))?;
        let _frame_admission =
            match Admission::acquire_limit(self.frame_admission.clone(), MAX_FRAME_WAITERS) {
                Ok(slot) => slot,
                Err(_) => {
                    return Err(NativeAdmissionError::Full(Box::new(
                        NativeAction::ObserverFrame { tab, attachment },
                    ))
                    .into());
                }
            };
        let _permit = self
            .frame_slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| {
                NativeAdmissionError::Stopped(Box::new(NativeAction::ObserverFrame {
                    tab,
                    attachment: attachment.clone(),
                }))
            })?;
        let NativeResponse::Frame(frame) = self
            .execute(NativeAction::Presentation {
                tab,
                attachment: NativeViewAttachment::Observer(attachment.clone()),
                action: NativePresentation::Frame { focused },
            })
            .await?
        else {
            anyhow::bail!("Service returned an invalid observer frame response.");
        };
        let Some(frame) = frame else {
            return Ok(None);
        };
        ensure!(
            frame.frame.identity.tab_id == tab
                && frame.frame.identity.generation == attachment.generation
                && matches!(frame.presentation.view, shellfox_runtime::PresentationLease::Observer(lease) if lease.viewer == attachment.viewer && lease.epoch == attachment.epoch),
            "Observer frame identity/view mismatch."
        );
        self.frames
            .lock()
            .map_err(|_| anyhow::anyhow!("Native frame cache poisoned."))?
            .accept(tab, true, frame.clone())?;
        if !frame.replay_lost {
            self.execute(NativeAction::AcceptObserverFrame {
                tab,
                attachment,
                frame_id: frame.frame_id,
            })
            .await?;
        }
        Ok(Some(frame))
    }
    pub fn latest(&self, tab: TabId, observer: bool) -> Result<Option<NativeFrame>> {
        Ok(self
            .frames
            .lock()
            .map_err(|_| anyhow::anyhow!("Native frame cache poisoned."))?
            .latest(tab, observer))
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
    pub async fn detach_all(&self) -> Result<()> {
        let local = self.execute(NativeAction::DetachAll).await;
        #[cfg(feature = "experimental-shellfox-native")]
        let peer = self.peers.close_window().await;
        local?;
        #[cfg(feature = "experimental-shellfox-native")]
        peer?;
        self.frames
            .lock()
            .map_err(|_| anyhow::anyhow!("Native frame cache poisoned."))?
            .clear();
        Ok(())
    }
}
