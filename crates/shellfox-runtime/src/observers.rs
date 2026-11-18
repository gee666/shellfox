//! Read-only credit metadata over Delivery's single shared output journal.
use crate::{delivery::Delivery, Chunk, ObserverLease, RuntimeError};
use std::collections::VecDeque;

const MAX_OBSERVERS: usize = 16;
const CREDIT_BYTES: usize = 128 * 1024;
const PENDING_CHUNKS: usize = 256;

#[derive(Debug)]
pub(crate) enum ObserverEvent {
    Output {
        lease: ObserverLease,
        chunk: Chunk,
    },
    ReplayLost {
        lease: ObserverLease,
        last_sequence: u64,
    },
    Exited {
        lease: ObserverLease,
        last_sequence: u64,
    },
}

struct View {
    lease: ObserverLease,
    initialized: bool,
    sent: u64,
    acked: u64,
    pending: VecDeque<(u64, usize)>,
    pending_bytes: usize,
    recovery_pending: Option<u64>,
    blocked: bool,
    loss_reported: bool,
    exit_sent: bool,
}

#[derive(Default)]
pub(crate) struct ObserverHub {
    views: Vec<View>,
    epoch: u64,
    next: usize,
}

fn replay_lost() -> RuntimeError {
    RuntimeError {
        code: crate::ErrorCode::ReplayLost,
        message: "Observer output replay expired; explicit ANSI recovery is required.".into(),
    }
}

impl ObserverHub {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// None requires explicit replay/recovery. Some(after) supplies a resume cursor.
    /// Reattaching one viewer fences its old lease without consuming another slot.
    pub(crate) fn observe(
        &mut self,
        viewer: u64,
        after: Option<u64>,
        journal: &Delivery,
    ) -> Result<ObserverLease, RuntimeError> {
        let existing = self
            .views
            .iter()
            .position(|view| view.lease.viewer == viewer);
        if existing.is_none() && self.views.len() >= MAX_OBSERVERS {
            return Err(RuntimeError::validation("At most 16 observers may attach."));
        }
        if after.is_some_and(|after| after > journal.current_sequence()) {
            return Err(RuntimeError::validation(
                "Observer cursor is ahead of output.",
            ));
        }
        let epoch = self
            .epoch
            .checked_add(1)
            .ok_or_else(|| RuntimeError::unavailable("Observer epoch exhausted."))?;
        let lease = ObserverLease { viewer, epoch };
        let view = View {
            lease,
            initialized: after.is_some(),
            sent: after.unwrap_or(0),
            acked: after.unwrap_or(0),
            pending: VecDeque::new(),
            pending_bytes: 0,
            recovery_pending: None,
            blocked: false,
            loss_reported: false,
            exit_sent: false,
        };
        if let Some(index) = existing {
            self.views[index] = view;
        } else {
            self.views.push(view);
        }
        self.epoch = epoch;
        Ok(lease)
    }

    pub(crate) fn check(&self, lease: ObserverLease) -> Result<(), RuntimeError> {
        if self.views.iter().any(|view| view.lease == lease) {
            Ok(())
        } else {
            Err(RuntimeError::control(
                "Current observer lease required.".into(),
            ))
        }
    }

    pub(crate) fn sent_sequence(&self, lease: ObserverLease) -> Option<u64> {
        self.views
            .iter()
            .find(|view| view.lease == lease)
            .map(|view| view.sent)
    }
    fn view(&mut self, lease: ObserverLease) -> Result<&mut View, RuntimeError> {
        self.views
            .iter_mut()
            .find(|view| view.lease == lease)
            .ok_or_else(|| RuntimeError::control("Current observer lease required.".into()))
    }

    /// Replay only initializes a cursor or resumes it. Pump commits queued chunks.
    pub(crate) fn replay(
        &mut self,
        lease: ObserverLease,
        after: u64,
        journal: &Delivery,
    ) -> Result<(), RuntimeError> {
        let view = self.view(lease)?;
        if after > journal.current_sequence() {
            return Err(RuntimeError::validation(
                "Observer cursor is ahead of output.",
            ));
        }
        if view.initialized && after != view.sent {
            return Err(RuntimeError::validation(
                "Replay must continue after the observer's sent sequence.",
            ));
        }
        if view.blocked
            || journal
                .first_sequence()
                .is_some_and(|first| after < first - 1)
        {
            view.initialized = true;
            view.blocked = true;
            return Err(replay_lost());
        }
        if !view.initialized {
            view.sent = after;
            view.acked = after;
            view.initialized = true;
        }
        Ok(())
    }

    pub(crate) fn acknowledge(
        &mut self,
        lease: ObserverLease,
        sequence: u64,
    ) -> Result<(), RuntimeError> {
        let view = self.view(lease)?;
        if sequence < view.acked || sequence > view.sent {
            return Err(RuntimeError::validation(
                "Observer ACK is outside its delivered range.",
            ));
        }
        view.acked = sequence;
        if view.recovery_pending == Some(sequence) {
            view.recovery_pending = None;
        }
        while view
            .pending
            .front()
            .is_some_and(|pending| pending.0 <= sequence)
        {
            if let Some((_, bytes)) = view.pending.pop_front() {
                view.pending_bytes -= bytes;
            }
        }
        Ok(())
    }

    /// Call after preparing this observer's recovery response at the journal barrier.
    /// The owner must queue that response before pumping. No controller state changes.
    pub(crate) fn recover(
        &mut self,
        lease: ObserverLease,
        journal: &Delivery,
    ) -> Result<u64, RuntimeError> {
        let view = self.view(lease)?;
        let barrier = journal.current_sequence();
        view.sent = barrier;
        view.pending.clear();
        view.pending_bytes = 0;
        view.initialized = true;
        view.recovery_pending = Some(barrier);
        view.blocked = false;
        view.loss_reported = false;
        Ok(barrier)
    }

    pub(crate) fn unobserve(&mut self, lease: ObserverLease) -> Result<(), RuntimeError> {
        let index = self
            .views
            .iter()
            .position(|view| view.lease == lease)
            .ok_or_else(|| RuntimeError::control("Current observer lease required.".into()))?;
        self.views.remove(index);
        if index < self.next {
            self.next -= 1;
        }
        if !self.views.is_empty() {
            self.next %= self.views.len();
        } else {
            self.next = 0;
        }
        Ok(())
    }

    /// One fair round, at most one event per view. true means accepted into the
    /// bounded retained owner outbox, not consumed by the client. Queue-full must
    /// return false and preserves all credit; channel-full retains that outbox.
    pub(crate) fn pump(
        &mut self,
        journal: &Delivery,
        exited: bool,
        mut emit: impl FnMut(ObserverEvent) -> bool,
    ) -> bool {
        let count = self.views.len();
        if count == 0 {
            return false;
        }
        let start = self.next % count;
        let mut queued = false;
        for offset in 0..count {
            let index = (start + offset) % count;
            let view = &mut self.views[index];
            if !view.initialized || view.recovery_pending.is_some() {
                continue;
            }
            if journal
                .first_sequence()
                .is_some_and(|first| view.sent < first - 1)
            {
                view.blocked = true;
            }
            let accepted = if view.blocked {
                if view.loss_reported {
                    continue;
                }
                let accepted = emit(ObserverEvent::ReplayLost {
                    lease: view.lease,
                    last_sequence: journal.current_sequence(),
                });
                view.loss_reported = accepted;
                accepted
            } else if let Some(chunk) = view
                .sent
                .checked_add(1)
                .and_then(|next| journal.chunk(next))
            {
                if view.pending.len() >= PENDING_CHUNKS
                    || view.pending_bytes + chunk.bytes.len() > CREDIT_BYTES
                {
                    continue;
                }
                let accepted = emit(ObserverEvent::Output {
                    lease: view.lease,
                    chunk: chunk.clone(),
                });
                if accepted {
                    view.sent = chunk.sequence;
                    view.pending.push_back((chunk.sequence, chunk.bytes.len()));
                    view.pending_bytes += chunk.bytes.len();
                }
                accepted
            } else if exited && !view.exit_sent && view.sent == journal.current_sequence() {
                let accepted = emit(ObserverEvent::Exited {
                    lease: view.lease,
                    last_sequence: view.sent,
                });
                view.exit_sent = accepted;
                accepted
            } else {
                false
            };
            // Full and available outbox slots can alternate under controller load.
            // Rejection must preserve the next observer's priority.
            if accepted {
                queued = true;
                self.next = (index + 1) % count;
            }
        }
        queued
    }
}

#[cfg(test)]
#[path = "observer_tests.rs"]
mod tests;
