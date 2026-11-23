//! Wakes the service owner loop when work arrives outside its own inbox.
use super::task::Task;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::SyncSender,
    Arc, OnceLock,
};

/// Coalesced: at most one wake message waits in the owner inbox, so a burst
/// of notifications can't crowd out real tasks.
#[derive(Clone)]
pub struct OwnerWake(Arc<Inner>);
struct Inner {
    pending: AtomicBool,
    sender: SyncSender<Task>,
}
impl OwnerWake {
    pub(super) fn new(sender: SyncSender<Task>) -> Self {
        Self(Arc::new(Inner {
            pending: AtomicBool::new(false),
            sender,
        }))
    }
    /// Never blocks. Safe from any thread.
    pub fn notify(&self) {
        if self.0.pending.swap(true, Ordering::AcqRel) {
            return;
        }
        // Full means queued tasks will run the loop anyway; Disconnected means stopped.
        let _ = self.0.sender.try_send(Task::Wake);
    }
    /// Called by the owner before each pass, so notifications during the
    /// pass queue a fresh wake.
    pub(super) fn rearm(&self) {
        self.0.pending.store(false, Ordering::Release);
    }
}

/// For sources created before the owner exists. Notifications before
/// binding are dropped: the owner runs a full pass after binding.
#[derive(Clone, Default)]
pub struct OwnerWakeSlot(Arc<OnceLock<OwnerWake>>);
impl OwnerWakeSlot {
    pub fn notify(&self) {
        if let Some(wake) = self.0.get() {
            wake.notify();
        }
    }
    pub(crate) fn bind(&self, wake: OwnerWake) {
        let _ = self.0.set(wake);
    }
    pub(crate) fn notifier(&self) -> Arc<dyn Fn() + Send + Sync> {
        let slot = self.clone();
        Arc::new(move || slot.notify())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_coalesce_until_rearmed() {
        let (sender, inbox) = std::sync::mpsc::sync_channel(4);
        let slot = OwnerWakeSlot::default();
        slot.notify();
        assert!(inbox.try_recv().is_err(), "unbound slot drops the wake");
        let wake = OwnerWake::new(sender);
        slot.bind(wake.clone());
        slot.notify();
        wake.notify();
        assert!(matches!(inbox.try_recv(), Ok(Task::Wake)));
        assert!(inbox.try_recv().is_err(), "second notify coalesced");
        wake.rearm();
        slot.notify();
        assert!(matches!(inbox.try_recv(), Ok(Task::Wake)));
    }
}
