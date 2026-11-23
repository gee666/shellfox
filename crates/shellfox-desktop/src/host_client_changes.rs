//! Host change streams. One read loop per kind serves the whole process; the
//! host holds each read until something changed, so the loops wait instead
//! of polling.
use super::{HostClient, attached::AttachedLocal};
use anyhow::Result;
use shellfox_service::{
    NativeTabActivity,
    local::{DesktopReply, DesktopRequest},
    power::{PowerReport, PowerRequest},
    remote::native::{RemoteControlState, RemoteRequest},
};
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;

/// Pause after a failed read, so a lost host isn't spun on.
const FAILED_READ: Duration = Duration::from_secs(1);

/// Yields the current value first, then each changed value.
pub(crate) struct Changes<T> {
    /// None until the first read returns.
    changes: watch::Receiver<Option<T>>,
    started: bool,
}

impl<T: Clone> Changes<T> {
    pub(crate) async fn next(&mut self) -> Result<T> {
        loop {
            let started = std::mem::replace(&mut self.started, true);
            if started || self.changes.borrow().is_none() {
                self.changes.changed().await?;
            }
            if let Some(value) = self.changes.borrow_and_update().clone() {
                return Ok(value);
            }
        }
    }
}

/// Process-wide attached read loops, shared by every window's client. They
/// run on their own thread so any caller can start them.
pub(super) struct Feeds {
    revision: watch::Receiver<Option<u64>>,
    activity: watch::Receiver<Option<Arc<[NativeTabActivity]>>>,
    power: watch::Receiver<Option<PowerReport>>,
    remote: watch::Receiver<Option<RemoteControlState>>,
}

impl Feeds {
    fn start(local: &AttachedLocal) -> Self {
        let mut loops = Vec::new();
        let revision = feed(
            local,
            &mut loops,
            PartialEq::eq,
            |local, last: Option<u64>| async move {
                let request = DesktopRequest::ChangedSince {
                    revision: last.unwrap_or(0),
                };
                match local.request(request).await? {
                    // The host's hold deadline passed without a change.
                    DesktopReply::Changed(None) => Ok(last),
                    DesktopReply::Changed(Some(event)) => Ok(Some(event.revision)),
                    _ => anyhow::bail!("Host returned an invalid change response."),
                }
            },
        );
        let activity = feed(local, &mut loops, PartialEq::eq, |local, _| async move {
            match local.request(DesktopRequest::ReadTabActivity).await? {
                DesktopReply::TabActivity(states) => Ok(Some(states.into())),
                _ => anyhow::bail!("Host returned an invalid tab activity response."),
            }
        });
        let power = feed(local, &mut loops, PartialEq::eq, |local, _| async move {
            Ok(Some(local.request(PowerRequest::Status {}).await?))
        });
        // The host answers remote state only when it changed or its hold expired.
        let remote = feed(
            local,
            &mut loops,
            |_, _| false,
            |local, _| async move { Ok(Some(local.remote(RemoteRequest::State).await?)) },
        );
        // The loops use Tokio timers, so they run inside this thread's runtime.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build();
        if let Ok(runtime) = runtime {
            let _ = std::thread::Builder::new()
                .name("shellfox-host-feeds".into())
                .spawn(move || {
                    runtime.block_on(async {
                        let mut running = tokio::task::JoinSet::new();
                        for read in loops {
                            running.spawn(read);
                        }
                        while running.join_next().await.is_some() {}
                    })
                });
        }
        Self {
            revision,
            activity,
            power,
            remote,
        }
    }
    /// The latest remote state, once the first read returned.
    pub(super) async fn remote_state(&self) -> Result<RemoteControlState> {
        let mut changes = self.remote.clone();
        let state = changes.wait_for(Option::is_some).await?;
        Ok(state.clone().expect("waited for a value"))
    }
}

type FeedLoop = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

/// Runs `read` until nobody observes it, publishing each changed value. The
/// host holds each read until it has something new.
fn feed<T, F, R>(
    local: &AttachedLocal,
    loops: &mut Vec<FeedLoop>,
    same: fn(&T, &T) -> bool,
    read: R,
) -> watch::Receiver<Option<T>>
where
    T: Clone + Send + Sync + 'static,
    F: std::future::Future<Output = Result<Option<T>>> + Send + 'static,
    R: Fn(AttachedLocal, Option<T>) -> F + Send + 'static,
{
    let (sender, receiver) = watch::channel(None);
    let local = local.clone();
    loops.push(Box::pin(async move {
        while !sender.is_closed() {
            let last = sender.borrow().clone();
            match read(local.clone(), last).await {
                Ok(value) => {
                    sender.send_if_modified(|current| {
                        let changed = match (&*current, &value) {
                            (Some(current), Some(value)) => !same(current, value),
                            (current, value) => current.is_some() != value.is_some(),
                        };
                        if changed {
                            *current = value;
                        }
                        changed
                    });
                }
                Err(_) => tokio::time::sleep(FAILED_READ).await,
            }
        }
    }));
    receiver
}

impl HostClient {
    pub(super) fn feeds(&self) -> &Feeds {
        self.feeds.get_or_init(|| Feeds::start(self.local()))
    }
    fn changes<T>(&self, pick: impl FnOnce(&Feeds) -> &watch::Receiver<Option<T>>) -> Changes<T> {
        let changes = pick(self.feeds()).clone();
        Changes {
            changes,
            started: false,
        }
    }
    /// Host metadata revisions (sessions, tabs, settings).
    pub(crate) fn revisions(&self) -> Changes<u64> {
        self.changes(|feeds| &feeds.revision)
    }
    pub(crate) fn tab_activity_changes(&self) -> Changes<Arc<[NativeTabActivity]>> {
        self.changes(|feeds| &feeds.activity)
    }
    pub(crate) fn power_changes(&self) -> Changes<PowerReport> {
        self.changes(|feeds| &feeds.power)
    }
    pub(crate) fn remote_changes(&self) -> Changes<RemoteControlState> {
        self.changes(|feeds| &feeds.remote)
    }
}
