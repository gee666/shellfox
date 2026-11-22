//! Holds an attached client's reads until they can return something new.
//! Each read is the ordinary request; the wire is unchanged. A held request
//! waits on the owner's watches here, in its own connection task, then goes
//! to the owner as usual. A deadline returns the current value.
use super::{request::PrivateRequest, DesktopRequest};
use crate::{
    native::{NativePresentation, NativeViewAttachment, NativeWatches},
    power::{PowerReport, PowerRequest},
    remote::native::{RemoteControlState, RemoteRequest},
    NativeTabActivity,
};
use shellfox_core::TabId;
use shellfox_protocol::CancellationToken;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::watch;

/// Below the 30 s private request deadline, including the owner's reply.
const HOLD: Duration = Duration::from_secs(25);
/// Bounds forgotten clients; a cleared entry only answers its next read at once.
const MAX_HELD: usize = 1024;

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
enum Kind {
    Frame { tab: TabId, observer: bool },
    Activity,
    Power,
    Remote,
}

/// What this client was last answered, as a receiver whose seen version
/// marks that answer.
enum Seen {
    Frame { changes: watch::Receiver<u64>, focused: bool },
    Activity(watch::Receiver<Arc<[NativeTabActivity]>>),
    Power(watch::Receiver<PowerReport>),
    Remote(watch::Receiver<RemoteControlState>),
}
impl Seen {
    fn has_changed(&self) -> bool {
        match self {
            Self::Frame { changes, .. } => changes.has_changed(),
            Self::Activity(changes) => changes.has_changed(),
            Self::Power(changes) => changes.has_changed(),
            Self::Remote(changes) => changes.has_changed(),
        }
        .unwrap_or(true)
    }
    /// A closed sender (tab or owner gone) returns at once; the owner reports why.
    async fn changed(&mut self) {
        let _ = match self {
            Self::Frame { changes, .. } => changes.changed().await,
            Self::Activity(changes) => changes.changed().await,
            Self::Power(changes) => changes.changed().await,
            Self::Remote(changes) => changes.changed().await,
        };
    }
    fn mark_seen(&mut self) {
        match self {
            Self::Frame { changes, .. } => {
                changes.borrow_and_update();
            }
            Self::Activity(changes) => {
                changes.borrow_and_update();
            }
            Self::Power(changes) => {
                changes.borrow_and_update();
            }
            Self::Remote(changes) => {
                changes.borrow_and_update();
            }
        }
    }
}

pub(super) struct LongPolls {
    watches: NativeWatches,
    remote: Option<watch::Receiver<RemoteControlState>>,
    seen: Mutex<HashMap<(uuid::Uuid, Kind), Seen>>,
}

impl LongPolls {
    pub fn new(
        watches: NativeWatches,
        remote: Option<watch::Receiver<RemoteControlState>>,
    ) -> Self {
        Self {
            watches,
            remote,
            seen: Mutex::default(),
        }
    }

    /// Waits until `request` from `client` would return something new, the
    /// deadline passes or `stop` fires. Requests that aren't reads pass through.
    pub async fn hold(&self, client: uuid::Uuid, request: &PrivateRequest, stop: &CancellationToken) {
        if let PrivateRequest::DesktopView(view) = request {
            if let DesktopRequest::ChangedSince { revision } = view.as_ref() {
                self.hold_revision(*revision, stop).await;
                return;
            }
        }
        self.forget(client, request);
        let Some((kind, focused)) = self.kind(request) else {
            return;
        };
        let key = (client, kind);
        let mut seen = {
            let mut held = self.lock();
            match held.remove(&key) {
                Some(seen) => seen,
                // A new reader is answered at once.
                None => match self.subscribe(kind, focused) {
                    Some(seen) => {
                        self.store(&mut held, key, seen);
                        return;
                    }
                    None => return,
                },
            }
        };
        let focus_changed = match &mut seen {
            Seen::Frame { focused: last, .. } => std::mem::replace(last, focused) != focused,
            _ => false,
        };
        if !focus_changed && !seen.has_changed() {
            tokio::select! {
                _ = tokio::time::timeout(HOLD, seen.changed()) => {}
                () = stop.cancelled() => {}
            }
        }
        seen.mark_seen();
        self.store(&mut self.lock(), key, seen);
    }

    async fn hold_revision(&self, seen: u64, stop: &CancellationToken) {
        let mut current = self.watches.revision.clone();
        let newer = current.wait_for(|value| *value > seen);
        tokio::select! {
            _ = tokio::time::timeout(HOLD, newer) => {}
            () = stop.cancelled() => {}
        }
    }

    fn kind(&self, request: &PrivateRequest) -> Option<(Kind, bool)> {
        match request {
            PrivateRequest::DesktopView(view) => match view.as_ref() {
                DesktopRequest::Presentation {
                    tab,
                    attachment,
                    action: NativePresentation::Frame { focused },
                } => Some((
                    Kind::Frame {
                        tab: *tab,
                        observer: matches!(attachment, NativeViewAttachment::Observer(_)),
                    },
                    *focused,
                )),
                DesktopRequest::ReadTabActivity => Some((Kind::Activity, false)),
                _ => None,
            },
            PrivateRequest::Power(PowerRequest::Status {}) => Some((Kind::Power, false)),
            PrivateRequest::RemoteOperator(RemoteRequest::State) => Some((Kind::Remote, false)),
            _ => None,
        }
    }

    /// The current value counts as seen: the request about to run returns it.
    fn subscribe(&self, kind: Kind, focused: bool) -> Option<Seen> {
        Some(match kind {
            Kind::Frame { tab, .. } => Seen::Frame {
                changes: self.watches.frames.subscribe(tab),
                focused,
            },
            Kind::Activity => Seen::Activity(self.watches.tab_activity.clone()),
            Kind::Power => Seen::Power(self.watches.power.clone()),
            Kind::Remote => Seen::Remote(self.remote.clone()?),
        })
        .map(|mut seen| {
            seen.mark_seen();
            seen
        })
    }

    /// A detached view starts over on its next attach.
    fn forget(&self, client: uuid::Uuid, request: &PrivateRequest) {
        let PrivateRequest::DesktopView(view) = request else {
            return;
        };
        let mut held = self.lock();
        match view.as_ref() {
            DesktopRequest::Detach { tab, .. } | DesktopRequest::Unobserve { tab, .. } => {
                held.retain(|(id, kind), _| {
                    *id != client || !matches!(kind, Kind::Frame { tab: held_tab, .. } if held_tab == tab)
                });
            }
            DesktopRequest::DetachAll => held.retain(|(id, _), _| *id != client),
            _ => {}
        }
    }

    fn store(&self, held: &mut HashMap<(uuid::Uuid, Kind), Seen>, key: (uuid::Uuid, Kind), seen: Seen) {
        if held.len() >= MAX_HELD {
            held.clear();
        }
        held.insert(key, seen);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(uuid::Uuid, Kind), Seen>> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Senders = (watch::Sender<u64>, watch::Sender<PowerReport>, watch::Sender<Arc<[NativeTabActivity]>>);
    fn polls() -> (LongPolls, Senders) {
        let (revision, revisions) = watch::channel(3);
        let power = watch::channel(PowerReport {
            status: crate::power::PowerStatus::Inactive,
            partial_reason: None,
            error: None,
            pending: false,
        });
        let activity = watch::channel(Arc::from([]));
        let watches = NativeWatches {
            revision: revisions,
            power: power.1,
            tab_activity: activity.1,
            frames: Default::default(),
        };
        (LongPolls::new(watches, None), (revision, power.0, activity.0))
    }
    fn activity() -> PrivateRequest {
        PrivateRequest::DesktopView(Box::new(DesktopRequest::ReadTabActivity))
    }
    async fn returns_soon(future: impl std::future::Future<Output = ()>) -> bool {
        tokio::time::timeout(Duration::from_millis(200), future).await.is_ok()
    }

    #[tokio::test]
    async fn a_reader_is_answered_at_once_then_held_until_a_change() {
        let (polls, _senders) = polls();
        let (client, stop) = (uuid::Uuid::new_v4(), CancellationToken::new());
        assert!(returns_soon(polls.hold(client, &activity(), &stop)).await);
        assert!(!returns_soon(polls.hold(client, &activity(), &stop)).await);
        // Another client has seen nothing yet.
        assert!(returns_soon(polls.hold(uuid::Uuid::new_v4(), &activity(), &stop)).await);
        stop.cancel();
        assert!(returns_soon(polls.hold(client, &activity(), &stop)).await);
    }

    #[tokio::test]
    async fn changed_since_waits_for_a_newer_revision() {
        let (polls, (revision, _power, _activity)) = polls();
        let (client, stop) = (uuid::Uuid::new_v4(), CancellationToken::new());
        let request = |revision| PrivateRequest::DesktopView(Box::new(DesktopRequest::ChangedSince { revision }));
        assert!(returns_soon(polls.hold(client, &request(2), &stop)).await);
        let current = request(3);
        let held = polls.hold(client, &current, &stop);
        tokio::pin!(held);
        assert!(!returns_soon(&mut held).await);
        revision.send_replace(4);
        assert!(returns_soon(held).await);
    }
}
