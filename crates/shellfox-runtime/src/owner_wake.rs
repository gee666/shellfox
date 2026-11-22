//! Wakes between the owner thread and its consumer, so neither side polls.
use crate::{Event, Shared};
use std::{
    cell::Cell,
    sync::{
        atomic::Ordering,
        mpsc::{SyncSender, TrySendError},
        Arc, Condvar, Mutex,
    },
    time::Instant,
};

/// Consumer callback, called from the owner thread after it queues events.
pub type EventWake = Arc<dyn Fn() + Send + Sync>;

/// Any number of notifications before the next wait collapse into one.
#[derive(Default)]
pub(crate) struct OwnerSignal {
    pending: Mutex<bool>,
    ready: Condvar,
}
impl OwnerSignal {
    pub fn notify(&self) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if !*pending {
            *pending = true;
            self.ready.notify_one();
        }
    }
    /// Blocks until notified or, when given, until `deadline`.
    pub fn wait(&self, deadline: Option<Instant>) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        while !*pending {
            match deadline {
                None => pending = self.ready.wait(pending).unwrap_or_else(|e| e.into_inner()),
                Some(deadline) => {
                    let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                        break;
                    };
                    pending = self
                        .ready
                        .wait_timeout(pending, left)
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }
            }
        }
        *pending = false;
    }
    pub fn waker(self: &Arc<Self>) -> shellfox_terminal::Wake {
        let signal = Arc::clone(self);
        Arc::new(move || signal.notify())
    }
}

/// What one owner round did with the event channel.
#[derive(Clone, Copy)]
pub(crate) struct Round {
    /// Something was queued. Emission is budgeted per round, so more may follow.
    pub sent: bool,
    /// A send found the channel full; the owner needs the consumer to make room.
    pub full: bool,
}

/// The owner's side of the event channel. Tracks, per owner round, whether
/// anything was queued (wake the consumer once) and whether the channel was
/// full (wait for the consumer to make room).
pub(crate) struct EventSender {
    inner: SyncSender<Event>,
    shared: Arc<Shared>,
    sent: Cell<bool>,
    full: Cell<bool>,
}
impl EventSender {
    pub fn new(inner: SyncSender<Event>, shared: Arc<Shared>) -> Self {
        Self {
            inner,
            shared,
            sent: Cell::new(false),
            full: Cell::new(false),
        }
    }
    pub fn try_send(&self, event: Event) -> Result<(), TrySendError<Event>> {
        let result = self.inner.try_send(event);
        match &result {
            Ok(()) => self.sent.set(true),
            Err(TrySendError::Full(_)) => self.full.set(true),
            Err(TrySendError::Disconnected(_)) => {}
        }
        result
    }
    /// Ends one owner round: wakes the consumer if events were queued.
    pub fn finish_round(&self) -> Round {
        let round = Round {
            sent: self.sent.take(),
            full: self.full.take(),
        };
        if round.sent {
            self.shared.wake_consumer();
        }
        round
    }
}

impl Shared {
    pub(crate) fn wake_consumer(&self) {
        let wake = self
            .event_wake
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(wake) = wake {
            wake();
        }
    }
    /// Asks the owner to clean up and exit, from outside the owner thread.
    pub(crate) fn abandon(&self) {
        self.abandoned.store(true, Ordering::Release);
        self.signal.notify();
    }
    /// Called by the consumer after it takes an event.
    pub(crate) fn event_taken(&self) {
        if self.needs_space.swap(false, Ordering::AcqRel) {
            self.signal.notify();
        }
    }
}

#[cfg(test)]
pub(crate) fn channel(
    shared: &Arc<Shared>,
    capacity: usize,
) -> (EventSender, std::sync::mpsc::Receiver<Event>) {
    let (sender, receiver) = std::sync::mpsc::sync_channel(capacity);
    (EventSender::new(sender, Arc::clone(shared)), receiver)
}
