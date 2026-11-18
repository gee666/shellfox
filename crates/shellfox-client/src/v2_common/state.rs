use shellfox_protocol::{
    observer_v2::{Discovery, Id},
    CancellationToken, TransportError,
};
use std::sync::Mutex;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Disconnected,
    Discovered,
    Connecting,
    Connected,
    LegacyAvailable,
    AwaitingApproval,
    Rejected,
    Reconnecting,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FallbackPolicy {
    Never,
    OfferLegacy,
}
#[derive(Clone, Debug)]
pub enum DiscoveryOutcome {
    Available(Discovery),
    Incompatible(Discovery),
    LegacyAvailable,
}
#[derive(Clone, Debug)]
pub enum ClientError<E> {
    InvalidRequest,
    InvalidResponse,
    Transport(TransportError),
    NotDiscovered,
    NotConnected,
    Busy,
    StaleConnection,
    ProfileDenied,
    Host(E),
}
impl<E: std::fmt::Debug> std::fmt::Display for ClientError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "V2 client error: {self:?}")
    }
}
impl<E: std::fmt::Debug> std::error::Error for ClientError<E> {}
pub(super) struct State<B> {
    pub client_id: Id,
    pub cycle: u64,
    pub cancel: CancellationToken,
    pub discovery: Option<Discovery>,
    pub binding: Option<B>,
    pub attempt: Option<Id>,
    pub pair_started: bool,
    pub pending: u8,
    pub polling: bool,
    pub negotiating: bool,
    pub status: Status,
}
impl<B> State<B> {
    pub fn invalidate(&mut self, status: Status) {
        self.cancel.cancel();
        self.cancel = CancellationToken::new();
        self.cycle += 1;
        self.binding = None;
        self.pending = 0;
        self.polling = false;
        self.negotiating = false;
        self.status = status;
    }
}
pub(super) struct CancelOnDrop(pub CancellationToken);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
#[derive(Clone, Copy)]
pub(super) enum Kind {
    Negotiate,
    Request,
    Poll,
}
pub(super) struct Guard<'a, B> {
    pub state: &'a Mutex<State<B>>,
    pub cycle: u64,
    pub kind: Kind,
    pub cancel: CancellationToken,
}
impl<B> Drop for Guard<'_, B> {
    fn drop(&mut self) {
        self.cancel.cancel();
        let mut s = self.state.lock().expect("v2 session state");
        if s.cycle != self.cycle {
            return;
        }
        match self.kind {
            Kind::Request => s.pending -= 1,
            Kind::Poll => s.polling = false,
            Kind::Negotiate => {
                s.negotiating = false;
                if s.status == Status::Connecting {
                    s.status = Status::Reconnecting;
                }
            }
        }
    }
}
