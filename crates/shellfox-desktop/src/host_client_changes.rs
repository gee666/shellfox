//! Host change streams. One read loop per kind serves the whole process; the
//! host holds each read until something changed, so the loops wait instead
//! of polling.
use super::{HostClient, attached::AttachedLocal};
use anyhow::Result;
use shellfox_service::{
    NativeTabActivity,
    local::{DesktopReply, DesktopRequest},
    power::{PowerReport, PowerRequest},
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

/// Process-wide attached read loops, shared by every window's client.
pub(super) struct Feeds {
    revision: watch::Receiver<Option<u64>>,
    activity: watch::Receiver<Option<Arc<[NativeTabActivity]>>>,
    power: watch::Receiver<Option<PowerReport>>,
}

impl Feeds {
    fn start(local: &AttachedLocal, executor: &gpui::BackgroundExecutor) -> Self {
        let revision = feed(local, executor, |local, last: Option<u64>| async move {
            let request = DesktopRequest::ChangedSince { revision: last.unwrap_or(0) };
            match local.request(request).await? {
                // The host's hold deadline passed without a change.
                DesktopReply::Changed(None) => Ok(last),
                DesktopReply::Changed(Some(event)) => Ok(Some(event.revision)),
                _ => anyhow::bail!("Host returned an invalid change response."),
            }
        });
        let activity = feed(local, executor, |local, _| async move {
            match local.request(DesktopRequest::ReadTabActivity).await? {
                DesktopReply::TabActivity(states) => Ok(Some(states.into())),
                _ => anyhow::bail!("Host returned an invalid tab activity response."),
            }
        });
        let power = feed(local, executor, |local, _| async move {
            Ok(Some(local.request(PowerRequest::Status {}).await?))
        });
        Self { revision, activity, power }
    }
}

/// Runs `read` forever, publishing each changed value. Each read is held by
/// the host until it has something new. Ends once nobody can observe it.
fn feed<T, F, R>(local: &AttachedLocal, executor: &gpui::BackgroundExecutor, read: R) -> watch::Receiver<Option<T>>
where
    T: Clone + PartialEq + Send + Sync + 'static,
    F: std::future::Future<Output = Result<Option<T>>> + Send + 'static,
    R: Fn(AttachedLocal, Option<T>) -> F + Send + 'static,
{
    let (sender, receiver) = watch::channel(None);
    let local = local.clone();
    let pause = executor.clone();
    executor
        .spawn(async move {
            while !sender.is_closed() {
                let last = sender.borrow().clone();
                match read(local.clone(), last).await {
                    Ok(value) => {
                        sender.send_if_modified(|current| {
                            let changed = *current != value;
                            if changed {
                                *current = value;
                            }
                            changed
                        });
                    }
                    Err(_) => pause.timer(FAILED_READ).await,
                }
            }
        })
        .detach();
    receiver
}

impl HostClient {
    async fn feeds(&self, executor: &gpui::BackgroundExecutor) -> Option<&Feeds> {
        let local = self.local()?;
        Some(self.feeds.get_or_init(|| async { Feeds::start(local, executor) }).await)
    }
    async fn changes<T>(
        &self,
        executor: gpui::BackgroundExecutor,
        pick: impl FnOnce(&Feeds) -> &watch::Receiver<Option<T>>,
    ) -> Changes<T> {
        let changes = match self.feeds(&executor).await {
            Some(feeds) => pick(feeds).clone(),
            // No attached host: a closed stream that yields nothing.
            None => watch::channel(None).1,
        };
        Changes { changes, started: false }
    }
    /// Host metadata revisions (sessions, tabs, settings).
    pub(crate) async fn revisions(&self, executor: gpui::BackgroundExecutor) -> Changes<u64> {
        self.changes(executor, |feeds| &feeds.revision).await
    }
    pub(crate) async fn tab_activity_changes(
        &self,
        executor: gpui::BackgroundExecutor,
    ) -> Changes<Arc<[NativeTabActivity]>> {
        self.changes(executor, |feeds| &feeds.activity).await
    }
    pub(crate) async fn power_changes(&self, executor: gpui::BackgroundExecutor) -> Changes<PowerReport> {
        self.changes(executor, |feeds| &feeds.power).await
    }
}
