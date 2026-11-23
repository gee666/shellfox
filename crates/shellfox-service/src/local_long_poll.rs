//! Holds an attached client's reads until they can return something new.
//! Each read is the ordinary request; the wire is unchanged. A held request
//! waits on the owner's watches here, in its own connection task, then goes
//! to the owner as usual. A deadline returns the current value.
use super::{request::PrivateRequest, DesktopRequest};
use crate::{
    desktop_launch::DesktopLaunchRequest,
    native::{NativePresentation, NativeViewAttachment, NativeWatches},
    power::{PowerReport, PowerRequest},
    remote::native::{RemoteControlState, RemoteRequest},
    NativeTabActivity,
};
use shellfox_core::{SessionId, TabId};
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
/// Reads one client may have waiting at once: one per terminal view plus its
/// change feeds. More are refused instead of piling up connection tasks.
const MAX_WAITING_PER_CLIENT: usize = 136;

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
enum Kind {
    Frame { tab: TabId, observer: bool },
    Activity,
    Power,
    Remote,
    Launch(SessionId),
    /// Terminal output for an attached CLI. Held only after an empty answer,
    /// since a batch may leave more output queued.
    Output(TabId),
    /// An attached desktop's next launch delivery; dispatch and completion both
    /// move the launch generation, so queued deliveries are released in turn.
    Delivery,
}

/// What this client was last answered, as a receiver whose seen version
/// marks that answer.
enum Seen {
    Frame { changes: watch::Receiver<u64>, focused: bool },
    Activity(watch::Receiver<Arc<[NativeTabActivity]>>),
    Power(watch::Receiver<PowerReport>),
    /// None, or a closed sender, when this host runs without remote access:
    /// the state then never changes.
    Remote(Option<watch::Receiver<RemoteControlState>>),
    Launch(watch::Receiver<u64>),
}
impl Seen {
    fn has_changed(&self) -> bool {
        match self {
            Self::Frame { changes, .. } => changes.has_changed(),
            Self::Activity(changes) => changes.has_changed(),
            Self::Power(changes) => changes.has_changed(),
            Self::Remote(changes) => Ok(changes.as_ref().is_some_and(|changes| changes.has_changed().unwrap_or(false))),
            Self::Launch(changes) => changes.has_changed(),
        }
        .unwrap_or(true)
    }
    /// A closed sender (tab or owner gone) returns at once; the owner reports why.
    async fn changed(&mut self) {
        let _ = match self {
            Self::Frame { changes, .. } => changes.changed().await,
            Self::Activity(changes) => changes.changed().await,
            Self::Power(changes) => changes.changed().await,
            Self::Remote(changes) => {
                if let Some(changes) = changes {
                    if changes.changed().await.is_ok() {
                        return;
                    }
                }
                std::future::pending().await
            }
            Self::Launch(changes) => changes.changed().await,
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
                if let Some(changes) = changes {
                    changes.borrow_and_update();
                }
            }
            Self::Launch(changes) => {
                changes.borrow_and_update();
            }
        }
    }
}

pub(super) struct LongPolls {
    watches: NativeWatches,
    remote: Option<watch::Receiver<RemoteControlState>>,
    seen: Mutex<HashMap<(uuid::Uuid, Kind), Seen>>,
    waiting: Arc<Mutex<HashMap<uuid::Uuid, usize>>>,
}

/// Counts one waiting read until dropped.
struct Waiting {
    client: uuid::Uuid,
    waiting: Arc<Mutex<HashMap<uuid::Uuid, usize>>>,
}
impl Drop for Waiting {
    fn drop(&mut self) {
        let mut waiting = self.waiting.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(count) = waiting.get_mut(&self.client) {
            *count -= 1;
            if *count == 0 {
                waiting.remove(&self.client);
            }
        }
    }
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
            waiting: Arc::default(),
        }
    }

    /// Waits until `request` from `client` would return something new, the
    /// deadline passes or `stop` fires. Requests that aren't reads pass through.
    /// Refused when this client already has too many reads waiting.
    pub async fn hold(
        &self,
        client: uuid::Uuid,
        request: &PrivateRequest,
        stop: &CancellationToken,
    ) -> anyhow::Result<()> {
        if let PrivateRequest::DesktopView(view) = request {
            if let DesktopRequest::ChangedSince { revision } = view.as_ref() {
                if *self.watches.revision.borrow() <= *revision {
                    let _waiting = self.wait_slot(client)?;
                    self.hold_revision(*revision, stop).await;
                }
                return Ok(());
            }
        }
        self.forget(client, request);
        let Some((kind, focused)) = self.kind(request) else {
            return Ok(());
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
                        return Ok(());
                    }
                    None => return Ok(()),
                },
            }
        };
        let focus_changed = match &mut seen {
            Seen::Frame { focused: last, .. } => std::mem::replace(last, focused) != focused,
            _ => false,
        };
        if !focus_changed && !seen.has_changed() {
            let _waiting = match self.wait_slot(client) {
                Ok(waiting) => waiting,
                Err(error) => {
                    self.store(&mut self.lock(), key, seen);
                    return Err(error);
                }
            };
            tokio::select! {
                _ = tokio::time::timeout(HOLD, seen.changed()) => {}
                () = stop.cancelled() => {}
            }
        }
        seen.mark_seen();
        self.store(&mut self.lock(), key, seen);
        Ok(())
    }

    fn wait_slot(&self, client: uuid::Uuid) -> anyhow::Result<Waiting> {
        let mut waiting = self.waiting.lock().unwrap_or_else(|e| e.into_inner());
        let count = waiting.entry(client).or_default();
        anyhow::ensure!(*count < MAX_WAITING_PER_CLIENT, "Too many reads are already waiting.");
        *count += 1;
        Ok(Waiting { client, waiting: self.waiting.clone() })
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
            PrivateRequest::DesktopLaunch(DesktopLaunchRequest::Receipt { request_id, .. }) => {
                Some((Kind::Launch(*request_id), false))
            }
            PrivateRequest::Experiment(crate::experiment::ExperimentRequest::Poll { tab, .. }) => {
                Some((Kind::Output(*tab), false))
            }
            PrivateRequest::DesktopLaunch(DesktopLaunchRequest::PollAttached { .. }) => {
                Some((Kind::Delivery, false))
            }
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
            Kind::Output(tab) => Seen::Frame {
                changes: self.watches.frames.subscribe(tab),
                focused,
            },
            Kind::Activity => Seen::Activity(self.watches.tab_activity.clone()),
            Kind::Power => Seen::Power(self.watches.power.clone()),
            Kind::Remote => Seen::Remote(self.remote.clone()),
            Kind::Launch(_) | Kind::Delivery => Seen::Launch(self.watches.launches.clone()),
        })
        .map(|mut seen| {
            seen.mark_seen();
            seen
        })
    }

    /// The tab of a CLI output read, which `answered_output` needs after the reply.
    pub fn output_tab(request: &PrivateRequest) -> Option<TabId> {
        match request {
            PrivateRequest::Experiment(crate::experiment::ExperimentRequest::Poll { tab, .. }) => Some(*tab),
            _ => None,
        }
    }
    /// After output was returned, the next read is answered at once: the
    /// batch may have left more queued.
    pub fn answered_output(&self, client: uuid::Uuid, tab: TabId, reply: &serde_json::Value) {
        let empty = reply.get("value").and_then(serde_json::Value::as_array).is_some_and(Vec::is_empty);
        if !empty {
            self.lock().remove(&(client, Kind::Output(tab)));
        }
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

    type Senders = (
        watch::Sender<u64>,
        watch::Sender<PowerReport>,
        watch::Sender<Arc<[NativeTabActivity]>>,
        watch::Sender<u64>,
    );
    fn polls() -> (LongPolls, Senders) {
        let (revision, revisions) = watch::channel(3);
        let power = watch::channel(PowerReport {
            status: crate::power::PowerStatus::Inactive,
            partial_reason: None,
            error: None,
            pending: false,
        });
        let activity = watch::channel(Arc::from([]));
        let launches = watch::channel(0);
        let watches = NativeWatches {
            revision: revisions,
            power: power.1,
            tab_activity: activity.1,
            frames: Default::default(),
            launches: launches.1,
        };
        (LongPolls::new(watches, None), (revision, power.0, activity.0, launches.0))
    }
    fn activity() -> PrivateRequest {
        PrivateRequest::DesktopView(Box::new(DesktopRequest::ReadTabActivity))
    }
    async fn returns_soon(future: impl std::future::Future<Output = anyhow::Result<()>>) -> bool {
        tokio::time::timeout(Duration::from_millis(200), future)
            .await
            .is_ok_and(|result| result.is_ok())
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
    async fn remote_state_is_held_when_remote_access_is_off() {
        let (polls, _senders) = polls();
        let (client, stop) = (uuid::Uuid::new_v4(), CancellationToken::new());
        let state = || PrivateRequest::RemoteOperator(RemoteRequest::State);
        assert!(returns_soon(polls.hold(client, &state(), &stop)).await);
        assert!(!returns_soon(polls.hold(client, &state(), &stop)).await);
    }

    #[tokio::test]
    async fn changed_since_waits_for_a_newer_revision() {
        let (polls, (revision, _power, _activity, _launches)) = polls();
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

    #[tokio::test]
    async fn a_client_cannot_pile_up_waiting_reads() {
        let (polls, _senders) = polls();
        let polls = Arc::new(polls);
        let (client, stop) = (uuid::Uuid::new_v4(), CancellationToken::new());
        let frame = |tab| {
            PrivateRequest::DesktopView(Box::new(DesktopRequest::Presentation {
                tab,
                attachment: NativeViewAttachment::Observer(crate::native::NativeObserverAttachment {
                    viewer: 1,
                    epoch: 1,
                    generation: shellfox_core::Generation::new(),
                }),
                action: NativePresentation::Frame { focused: true },
            }))
        };
        let tabs: Vec<_> = (0..=MAX_WAITING_PER_CLIENT).map(|_| TabId::new()).collect();
        let mut waiting = tokio::task::JoinSet::new();
        for tab in &tabs[..MAX_WAITING_PER_CLIENT] {
            polls.hold(client, &frame(*tab), &stop).await.unwrap();
            let (polls, stop, request) = (polls.clone(), stop.clone(), frame(*tab));
            waiting.spawn(async move { polls.hold(client, &request, &stop).await });
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        let over = tabs[MAX_WAITING_PER_CLIENT];
        polls.hold(client, &frame(over), &stop).await.unwrap();
        assert!(polls.hold(client, &frame(over), &stop).await.is_err());
        // Other clients are unaffected.
        assert!(returns_soon(polls.hold(uuid::Uuid::new_v4(), &frame(over), &stop)).await);
        stop.cancel();
        while let Some(result) = waiting.join_next().await {
            assert!(result.unwrap().is_ok());
        }
    }

    #[tokio::test]
    async fn a_launch_receipt_read_waits_for_a_receipt_change() {
        let (polls, (_revision, _power, _activity, launches)) = polls();
        let (client, stop) = (uuid::Uuid::new_v4(), CancellationToken::new());
        let receipt = PrivateRequest::DesktopLaunch(DesktopLaunchRequest::Receipt {
            owner: "11111111-1111-4111-8111-111111111111".parse().unwrap(),
            request_id: SessionId::new(),
        });
        assert!(returns_soon(polls.hold(client, &receipt, &stop)).await);
        let held = polls.hold(client, &receipt, &stop);
        tokio::pin!(held);
        assert!(!returns_soon(&mut held).await);
        launches.send_replace(1);
        assert!(returns_soon(held).await);
    }
}
