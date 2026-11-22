//! Host change streams. In process they await the owner's watches. An
//! attached desktop sends the ordinary request, and its host holds the reply
//! until something changed since the last reply to this client.
use super::{HostClient, NativeAction, NativeResponse};
use anyhow::Result;
use shellfox_service::{NativeTabActivity, power::PowerReport};
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;

// Attached hosts answer these reads at once for now; pace them like the old timers.
const REVISION_PACE: Duration = Duration::from_millis(250);
const ACTIVITY_PACE: Duration = Duration::from_millis(250);
const POWER_PACE: Duration = Duration::from_secs(1);

/// Yields the current value first, then each changed value.
pub(crate) struct Changes<T> {
    client: HostClient,
    executor: gpui::BackgroundExecutor,
    local: Option<watch::Receiver<T>>,
    started: bool,
    last: Option<T>,
}

impl HostClient {
    /// Host metadata revisions (sessions, tabs, settings).
    pub(crate) async fn revisions(&self, executor: gpui::BackgroundExecutor) -> Changes<u64> {
        let local = self.watches().await.map(|watches| watches.revision.clone());
        Changes::new(self, executor, local)
    }
    pub(crate) async fn tab_activity_changes(
        &self,
        executor: gpui::BackgroundExecutor,
    ) -> Changes<Arc<[NativeTabActivity]>> {
        let local = self.watches().await.map(|watches| watches.tab_activity.clone());
        Changes::new(self, executor, local)
    }
    pub(crate) async fn power_changes(&self, executor: gpui::BackgroundExecutor) -> Changes<PowerReport> {
        let local = self.watches().await.map(|watches| watches.power.clone());
        Changes::new(self, executor, local)
    }
}

impl<T: Clone> Changes<T> {
    fn new(client: &HostClient, executor: gpui::BackgroundExecutor, local: Option<watch::Receiver<T>>) -> Self {
        Self { client: client.clone(), executor, local, started: false, last: None }
    }
    /// Waits before every remote read except the first.
    async fn pace(&mut self, duration: Duration) {
        if std::mem::replace(&mut self.started, true) {
            self.executor.timer(duration).await;
        }
    }
    /// None when the host stopped publishing.
    async fn next_local(&mut self) -> Option<Option<T>> {
        let local = self.local.as_mut()?;
        if std::mem::replace(&mut self.started, true) && local.changed().await.is_err() {
            return Some(None);
        }
        Some(Some(local.borrow_and_update().clone()))
    }
}

impl Changes<u64> {
    pub(crate) async fn next(&mut self) -> Result<u64> {
        if let Some(value) = self.next_local().await {
            return value.ok_or_else(stopped);
        }
        let seen = self.last.unwrap_or(0);
        loop {
            self.pace(REVISION_PACE).await;
            // A failed read is retried; the stream ends only with the host.
            if let Ok(NativeResponse::Changed(Some(event))) =
                self.client.execute(NativeAction::ChangedSince { revision: seen }).await
            {
                self.last = Some(event.revision);
                return Ok(event.revision);
            }
        }
    }
}

impl Changes<Arc<[NativeTabActivity]>> {
    pub(crate) async fn next(&mut self) -> Result<Arc<[NativeTabActivity]>> {
        if let Some(value) = self.next_local().await {
            return value.ok_or_else(stopped);
        }
        loop {
            self.pace(ACTIVITY_PACE).await;
            if let Ok(NativeResponse::TabActivity(states)) =
                self.client.execute(NativeAction::ReadTabActivity).await
            {
                return Ok(states.into());
            }
        }
    }
}

impl Changes<PowerReport> {
    pub(crate) async fn next(&mut self) -> Result<PowerReport> {
        if let Some(value) = self.next_local().await {
            return value.ok_or_else(stopped);
        }
        loop {
            self.pace(POWER_PACE).await;
            let Ok(report) = self.client.power_status().await else {
                continue;
            };
            if self.last.as_ref() != Some(&report) {
                self.last = Some(report.clone());
                return Ok(report);
            }
        }
    }
}

fn stopped() -> anyhow::Error {
    anyhow::anyhow!("The host stopped publishing changes.")
}
