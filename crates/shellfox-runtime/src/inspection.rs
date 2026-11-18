//! Bounded, nonblocking runtime access to a separate read-only inspection worker.
use crate::Identity;
pub use shellfox_terminal::inspection::InspectionFailure;
use shellfox_terminal::inspection::{InspectionCapability, RawOwnedEvidence};
use std::sync::{
    mpsc::{self, Receiver, SyncSender, TrySendError},
    Arc, Mutex, TryLockError,
};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InspectionTicket {
    identity: Identity,
    sequence: u64,
}
pub struct ProcessInspection {
    pub identity: Identity,
    pub evidence: RawOwnedEvidence,
}
pub enum InspectionPoll {
    Pending,
    Ready(Result<ProcessInspection, InspectionFailure>),
}
struct State {
    identity: Identity,
    next: u64,
    pending: Option<InspectionTicket>,
    result: Option<(
        InspectionTicket,
        Result<RawOwnedEvidence, InspectionFailure>,
    )>,
    capability: Option<InspectionCapability>,
    availability: Option<InspectionFailure>,
    stopped: bool,
}
pub(crate) struct InspectionClient {
    requests: SyncSender<InspectionTicket>,
    shared: Arc<Mutex<State>>,
}
pub(crate) struct InspectionPublisher {
    shared: Arc<Mutex<State>>,
}
pub(crate) fn start(
    identity: Identity,
) -> std::io::Result<(InspectionClient, InspectionPublisher)> {
    let (requests, incoming) = mpsc::sync_channel(1);
    let shared = Arc::new(Mutex::new(State {
        identity,
        next: 0,
        pending: None,
        result: None,
        capability: None,
        availability: None,
        stopped: false,
    }));
    let worker_state = Arc::clone(&shared);
    std::thread::Builder::new()
        .name("shellfox-owned-inspection".into())
        .spawn(move || worker(incoming, worker_state))?;
    Ok((
        InspectionClient {
            requests,
            shared: Arc::clone(&shared),
        },
        InspectionPublisher { shared },
    ))
}
fn worker(incoming: Receiver<InspectionTicket>, shared: Arc<Mutex<State>>) {
    loop {
        let ticket = match incoming.recv_timeout(Duration::from_millis(100)) {
            Ok(ticket) => ticket,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if shared.lock().unwrap_or_else(|e| e.into_inner()).stopped {
                    return;
                }
                continue;
            }
        };
        let capability = {
            let state = shared.lock().unwrap_or_else(|e| e.into_inner());
            if state.stopped {
                return;
            }
            if state.identity != ticket.identity || state.pending != Some(ticket) {
                continue;
            }
            state.capability.clone()
        };
        // No terminal/parser/channel/state lock is held across native inspection.
        let result = capability
            .ok_or(InspectionFailure::Unavailable)
            .and_then(|capability| capability.capture());
        let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped {
            return;
        }
        if state.identity == ticket.identity && state.pending == Some(ticket) {
            state.result = Some((ticket, result));
        }
    }
}
impl InspectionClient {
    pub(crate) fn request(&self) -> Result<InspectionTicket, InspectionFailure> {
        let mut state = match self.shared.try_lock() {
            Ok(state) => state,
            Err(TryLockError::WouldBlock) => return Err(InspectionFailure::Pending),
            Err(TryLockError::Poisoned(_)) => return Err(InspectionFailure::Unavailable),
        };
        if state.stopped {
            return Err(InspectionFailure::Unavailable);
        }
        if let Some(error) = state.availability {
            return Err(error);
        }
        if state.capability.is_none() || state.pending.is_some() {
            return Err(InspectionFailure::Pending);
        }
        let sequence = state.next.checked_add(1).ok_or(InspectionFailure::Bounds)?;
        let ticket = InspectionTicket {
            identity: state.identity,
            sequence,
        };
        state.pending = Some(ticket);
        match self.requests.try_send(ticket) {
            Ok(()) => {
                state.next = sequence;
                Ok(ticket)
            }
            Err(TrySendError::Full(_)) => {
                state.pending = None;
                Err(InspectionFailure::Pending)
            }
            Err(TrySendError::Disconnected(_)) => {
                state.pending = None;
                Err(InspectionFailure::Unavailable)
            }
        }
    }
    pub(crate) fn poll(&self, ticket: InspectionTicket) -> InspectionPoll {
        let mut state = match self.shared.try_lock() {
            Ok(state) => state,
            Err(TryLockError::WouldBlock) => return InspectionPoll::Pending,
            Err(TryLockError::Poisoned(_)) => {
                return InspectionPoll::Ready(Err(InspectionFailure::Unavailable))
            }
        };
        if state.identity != ticket.identity {
            return InspectionPoll::Ready(Err(InspectionFailure::OwnershipMismatch));
        }
        if state.stopped {
            return InspectionPoll::Ready(Err(InspectionFailure::Unavailable));
        }
        if state.pending != Some(ticket) {
            return InspectionPoll::Ready(Err(InspectionFailure::OwnershipMismatch));
        }
        match state.result.take() {
            Some((result_ticket, result)) if result_ticket == ticket => {
                state.pending = None;
                InspectionPoll::Ready(result.map(|evidence| ProcessInspection {
                    identity: ticket.identity,
                    evidence,
                }))
            }
            Some(result) => {
                state.result = Some(result);
                InspectionPoll::Pending
            }
            None => InspectionPoll::Pending,
        }
    }
    pub(crate) fn invalidate(&self) {
        invalidate(&self.shared);
    }
}
impl Drop for InspectionClient {
    fn drop(&mut self) {
        self.invalidate();
    }
}
impl InspectionPublisher {
    pub(crate) fn bind(&self, capability: Result<InspectionCapability, InspectionFailure>) {
        let mut state = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        if state.stopped {
            return;
        }
        match capability {
            Ok(capability) => state.capability = Some(capability),
            Err(error) => state.availability = Some(error),
        }
    }
    pub(crate) fn stop(&self) {
        invalidate(&self.shared);
    }
}
impl Drop for InspectionPublisher {
    fn drop(&mut self) {
        self.stop();
    }
}
fn invalidate(shared: &Mutex<State>) {
    let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
    state.stopped = true;
    state.capability = None;
    state.pending = None;
    state.result = None;
}
#[cfg(test)]
#[path = "inspection_tests.rs"]
mod tests;
