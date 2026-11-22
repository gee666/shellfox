//! Reachable bounded records. Notifications never carry or own parser handles.
use crate::*;
use std::{
    io,
    sync::mpsc::{self, SyncSender},
    thread::JoinHandle,
    time::Duration,
};
const MAX_RECORDS: usize = 64;
static REGISTRY: Mutex<Option<Arc<Registry>>> = Mutex::new(None);
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExternalCleanupReport {
    pub reserved: usize,
    pub pending: usize,
    pub joined: usize,
    pub notification_failed: bool,
}
pub(crate) struct Slot {
    owner: Mutex<Option<JoinHandle<()>>>,
    complete: AtomicBool,
    status: Arc<Mutex<ExternalStatus>>,
}
impl Slot {
    pub(crate) fn joined(&self) {
        self.status
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .closed = true;
        self.complete.store(true, Ordering::Release);
    }
    fn retry(&self) -> bool {
        let owner = {
            let mut owned = self.owner.lock().unwrap_or_else(|error| error.into_inner());
            if owned.as_ref().is_none_or(|owner| !owner.is_finished()) {
                return false;
            }
            owned.take()
        };
        if let Some(owner) = owner {
            let _ = owner.join();
            self.joined();
            return true;
        }
        false
    }
}
const CLEANUP_RETRY: Duration = Duration::from_millis(20);
pub(crate) struct Registry {
    slots: Mutex<Vec<Arc<Slot>>>,
    wake: SyncSender<()>,
    notification_failed: AtomicBool,
}
impl Registry {
    fn reserve(self: &Arc<Self>, status: Arc<Mutex<ExternalStatus>>) -> io::Result<Reservation> {
        let mut slots = self.slots.lock().unwrap_or_else(|error| error.into_inner());
        slots.retain(|slot| !slot.complete.load(Ordering::Acquire));
        if slots.len() >= MAX_RECORDS {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "External parser cleanup record limit reached.",
            ));
        }
        let slot = Arc::new(Slot {
            owner: Mutex::new(None),
            complete: AtomicBool::new(false),
            status,
        });
        slots.push(Arc::clone(&slot));
        Ok(Reservation {
            registry: Arc::clone(self),
            slot,
            bound: false,
        })
    }
    fn retry(&self) -> ExternalCleanupReport {
        let slots = self
            .slots
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let mut joined = 0;
        for slot in &slots {
            joined += usize::from(slot.retry());
        }
        let pending = slots
            .iter()
            .filter(|slot| {
                slot.owner
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .is_some()
            })
            .count();
        let reserved = slots
            .iter()
            .filter(|slot| !slot.complete.load(Ordering::Acquire))
            .count();
        ExternalCleanupReport {
            reserved,
            pending,
            joined,
            notification_failed: self.notification_failed.load(Ordering::Acquire),
        }
    }
    fn notify(&self) {
        if self.wake.try_send(()).is_err() {
            self.notification_failed.store(true, Ordering::Release);
        }
    }
}
pub(crate) struct Reservation {
    registry: Arc<Registry>,
    slot: Arc<Slot>,
    bound: bool,
}
impl Reservation {
    pub(crate) fn bind(mut self, owner: JoinHandle<()>) -> super::ExternalControl {
        self.bound = true;
        super::ExternalControl {
            owner: Some(owner),
            record: self,
        }
    }
    pub(crate) fn handoff(&self, owner: JoinHandle<()>) {
        // Unique, non-clone Reservation binds one controller; its Drop moves the
        // handle here once. Full wakeups can never overwrite/drop that handle.
        *self
            .slot
            .owner
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(owner);
        self.registry.notify();
    }
    pub(crate) fn joined(&self) {
        self.slot.joined();
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if !self.bound {
            self.slot.joined();
        }
    }
}
pub(crate) fn reserve(status: Arc<Mutex<ExternalStatus>>) -> io::Result<Reservation> {
    let mut global = REGISTRY.lock().unwrap_or_else(|error| error.into_inner());
    if global.is_none() {
        let (wake, incoming) = mpsc::sync_channel(1);
        let registry = Arc::new(Registry {
            slots: Mutex::new(Vec::new()),
            wake,
            notification_failed: AtomicBool::new(false),
        });
        let worker = Arc::clone(&registry);
        std::thread::Builder::new()
            .name("shellfox-external-cleanup".into())
            .spawn(move || {
                // Retry on a timer only while a handed-off owner is still running.
                let mut pending = false;
                loop {
                    let woke = if pending {
                        match incoming.recv_timeout(CLEANUP_RETRY) {
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                            _ => true,
                        }
                    } else {
                        incoming.recv().is_ok()
                    };
                    if !woke {
                        return;
                    }
                    pending = worker.retry().pending > 0;
                }
            })?;
        *global = Some(registry);
    }
    global
        .as_ref()
        .ok_or_else(|| io::Error::other("External cleanup registry unavailable."))?
        .reserve(status)
}
/// Also works if the background wakeup/worker failed. Every original handle is
/// in its pre-reserved reachable record until a completed join proves closure.
pub fn poll_external_cleanup() -> ExternalCleanupReport {
    REGISTRY
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
        .map_or_else(ExternalCleanupReport::default, |registry| registry.retry())
}
#[cfg(test)]
#[path = "external_cleanup_tests.rs"]
mod tests;
