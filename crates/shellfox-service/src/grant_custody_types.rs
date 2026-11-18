use shellfox_runtime::{Identity, Lease, ObserverLease, RequestId, RuntimeError};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use uuid::Uuid;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GrantKind {
    Controller(u64),
    Observer(u64),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActualGrant {
    Controller(Lease),
    Observer(ObserverLease),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GrantTicket {
    pub(super) identity: Identity,
    pub(super) request: RequestId,
    pub(super) nonce: Uuid,
}
impl GrantTicket {
    pub fn identity(self) -> Identity {
        self.identity
    }
    pub fn request(self) -> RequestId {
        self.request
    }
}
#[derive(Debug, Clone)]
pub(crate) enum GrantPoll {
    Pending,
    Granted(ActualGrant),
    Rejected(RuntimeError),
    Retiring,
    Retired,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CustodyError {
    Full,
    Conflict,
    Stale,
    NotSent,
    ReleaseSent,
    Cancelled,
    WrongReply,
}
/// Dropping/cancelling the recipient updates the ledger-owned cell directly.
/// Queue Full can never lose abandonment. This is not a Runtime cancellation.
pub(crate) struct GrantGuard {
    pub(super) abandoned: Arc<AtomicBool>,
    pub(super) armed: bool,
}
impl GrantGuard {
    pub fn disarm(&mut self) {
        self.armed = false;
    }
}
impl Drop for GrantGuard {
    fn drop(&mut self) {
        if self.armed {
            self.abandoned.store(true, Ordering::Release);
        }
    }
}
