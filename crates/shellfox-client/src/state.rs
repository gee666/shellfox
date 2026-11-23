use shellfox_protocol::*;
use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    AwaitingApproval,
    Rejected,
    Connected,
    Reconnecting,
    Suspended,
}
#[derive(Clone, Debug)]
pub struct ClientState {
    pub status: ConnectionStatus,
    pub permission: Permission,
    /// Changes on every successful pairing. Discard all view cursors after a change.
    pub connection_id: u64,
    pub last_error: Option<AppError>,
    pub retry_after: Option<Duration>,
    /// Counts polls that reported a host change (sessions, settings or tab
    /// activity). A catalog read is due when it differs from the value seen
    /// at the last read.
    pub catalog: u64,
}
#[derive(Clone, Debug)]
pub enum ClientError {
    Validation(ValidationError),
    Transport(TransportError),
    NotConnected,
    StaleConnection,
    TooManyRequests,
}
impl From<ValidationError> for ClientError {
    fn from(error: ValidationError) -> Self {
        Self::Validation(error)
    }
}
impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Shellfox client error: {self:?}")
    }
}
impl std::error::Error for ClientError {}
pub(crate) struct Session {
    pub(crate) state: ClientState,
    pub(crate) client_id: ClientId,
    pub(crate) cycle: u64,
    pub(crate) cancel: CancellationToken,
    pub(crate) failures: u32,
    pub(crate) pending: u8,
    pub(crate) polling: bool,
    pub(crate) pairing: bool,
}
#[derive(Clone, Copy)]
pub(crate) enum CallKind {
    Pair,
    Poll,
    Request,
}
pub(crate) type Wake = Arc<dyn Fn() + Send + Sync>;
/// Optional callback for ClientState changes that matter to a view.
#[derive(Default)]
pub(crate) struct StateWaker(Mutex<Option<Wake>>);
impl StateWaker {
    pub(crate) fn set(&self, wake: Option<Wake>) {
        *self.0.lock().expect("client waker") = wake;
    }
}
/// Session lock that calls the waker after the lock is released, if status,
/// permission, connection_id or catalog changed while it was held.
pub(crate) struct Locked<'a> {
    guard: Option<MutexGuard<'a, Session>>,
    before: (ConnectionStatus, Permission, u64, u64),
    waker: &'a StateWaker,
}
fn key(s: &Session) -> (ConnectionStatus, Permission, u64, u64) {
    (s.state.status, s.state.permission, s.state.connection_id, s.state.catalog)
}
pub(crate) fn lock<'a>(session: &'a Mutex<Session>, waker: &'a StateWaker) -> Locked<'a> {
    let guard = session.lock().expect("client state");
    Locked {
        before: key(&guard),
        guard: Some(guard),
        waker,
    }
}
impl Deref for Locked<'_> {
    type Target = Session;
    fn deref(&self) -> &Session {
        self.guard.as_ref().expect("held until drop")
    }
}
impl DerefMut for Locked<'_> {
    fn deref_mut(&mut self) -> &mut Session {
        self.guard.as_mut().expect("held until drop")
    }
}
impl Drop for Locked<'_> {
    fn drop(&mut self) {
        let Some(guard) = self.guard.take() else {
            return;
        };
        let changed = key(&guard) != self.before;
        drop(guard);
        if changed {
            let wake = self.waker.0.lock().ok().and_then(|wake| wake.clone());
            if let Some(wake) = wake {
                wake();
            }
        }
    }
}
pub(crate) struct CallGuard<'a> {
    pub(crate) session: &'a Mutex<Session>,
    pub(crate) waker: &'a StateWaker,
    pub(crate) cycle: u64,
    pub(crate) kind: CallKind,
    pub(crate) cancel: CancellationToken,
}
impl Drop for CallGuard<'_> {
    fn drop(&mut self) {
        self.cancel.cancel();
        let mut s = lock(self.session, self.waker);
        if s.cycle != self.cycle {
            return;
        }
        match self.kind {
            CallKind::Request => s.pending -= 1,
            CallKind::Poll => s.polling = false,
            CallKind::Pair => {
                s.pairing = false;
                if s.state.status == ConnectionStatus::Connecting {
                    s.state.status = ConnectionStatus::Reconnecting;
                    s.state.retry_after = Some(Duration::from_secs(1));
                }
            }
        }
    }
}
