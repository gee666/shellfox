//! Private local launch intents. No GUI, second Host, PTY or public/mobile route.
#[path = "desktop_launch_delivery.rs"]
mod delivery;
#[path = "desktop_launch_local.rs"]
mod local;
#[path = "desktop_launch_router.rs"]
mod router;
#[path = "desktop_launch_service.rs"]
mod service;
#[path = "desktop_launch_types.rs"]
mod types;
pub use service::{folder_profile, LaunchDirectoryPolicy, ServiceLaunchResolver};
#[cfg(test)]
#[path = "desktop_launch_tests.rs"]
mod tests;
pub use local::*;
use shellfox_core::SessionId;
use std::collections::{BTreeMap, VecDeque};
use tokio::sync::mpsc;
pub use types::*;
use uuid::Uuid;

pub struct DesktopRegistration {
    pub token: DesktopClientToken,
    pub receiver: mpsc::Receiver<DesktopLaunchDelivery>,
}
impl DesktopRegistration {
    /// Native adapter polls one bounded delivery. Poll is not authorization;
    /// claim must recheck owner/client/receipt before any UI effect.
    pub fn poll_delivery(&mut self) -> Result<Option<DesktopLaunchDelivery>, LaunchError> {
        match self.receiver.try_recv() {
            Ok(delivery) => Ok(Some(delivery)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => Err(LaunchError::StaleClient),
        }
    }
}
struct Registered {
    token: DesktopClientToken,
    sender: mpsc::Sender<DesktopLaunchDelivery>,
}
struct Entry {
    intent: DesktopLaunchIntent,
    kind: LaunchRequestKind,
    target: Option<LaunchTarget>,
    client: Option<DesktopClientToken>,
    state: LaunchState,
}
/// One instance lives beside the existing Service on its owner thread. Receipt
/// exhaustion rejects admission instead of silently evicting deduplication IDs.
pub struct DesktopLaunchRouter {
    owner: LaunchOwnerFence,
    live: bool,
    desktop: Option<Registered>,
    pending: VecDeque<SessionId>,
    entries: BTreeMap<SessionId, Entry>,
    /// Bumped whenever a receipt may have changed state; published to held receipt reads.
    generation: u64,
}
impl Default for DesktopLaunchRouter {
    fn default() -> Self {
        Self::new()
    }
}
impl DesktopLaunchRouter {
    pub fn new() -> Self {
        Self {
            owner: LaunchOwnerFence(Uuid::new_v4()),
            live: true,
            desktop: None,
            pending: VecDeque::new(),
            entries: BTreeMap::new(),
            generation: 0,
        }
    }
    pub fn owner(&self) -> LaunchOwnerFence {
        self.owner
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    fn receipts_changed(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }
    fn check(
        &mut self,
        owner: LaunchOwnerFence,
        authority: &impl LaunchAuthority,
    ) -> Result<(), LaunchError> {
        if !self.live || owner != self.owner {
            return Err(LaunchError::StaleOwner);
        }
        if authority.check_owned().is_err() {
            self.invalidate_owner();
            return Err(LaunchError::StaleOwner);
        }
        Ok(())
    }
    pub fn invalidate_owner(&mut self) {
        self.receipts_changed();
        self.live = false;
        self.desktop = None;
        self.pending.clear();
        for entry in self.entries.values_mut() {
            entry.state = match entry.state {
                LaunchState::Queued => LaunchState::Failed(LaunchError::StaleOwner),
                LaunchState::Dispatched | LaunchState::Applying => LaunchState::Uncertain,
                state => state,
            };
        }
    }
}
