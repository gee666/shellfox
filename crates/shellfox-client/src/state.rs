use shellfox_protocol::*;
use std::{sync::Mutex, time::Duration};

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
pub(crate) struct CallGuard<'a> {
    pub(crate) session: &'a Mutex<Session>,
    pub(crate) cycle: u64,
    pub(crate) kind: CallKind,
    pub(crate) cancel: CancellationToken,
}
impl Drop for CallGuard<'_> {
    fn drop(&mut self) {
        self.cancel.cancel();
        let mut s = self.session.lock().expect("client state");
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
