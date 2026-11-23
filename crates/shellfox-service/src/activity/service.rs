use crate::activity::integration::{
    ActivityClock, ActivityPersistence, ActivityUtcAnchor, OwnerActivity, SourceFactsPublisher,
    SourceFactsWatch,
};
use anyhow::{Context, Result};
use shellfox_core::SessionId;
use std::collections::HashSet;
mod visibility;
use shellfox_core::SettingsProcessRule;
use std::{
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub(crate) struct ServiceActivity {
    pub owner: OwnerActivity,
    pub persistence: ActivityPersistence,
    pub publisher: SourceFactsPublisher,
    pub watch: SourceFactsWatch,
    pub rules: Vec<SettingsProcessRule>,
    pub next_poll_ms: u64,
    pub dirty: bool,
    visibility: visibility::Visibility,
}
impl ServiceActivity {
    pub(crate) fn report_desktop_visibility(&mut self, controller: u64, ids: Vec<SessionId>) {
        self.visibility.desktop(controller, ids);
    }

    pub(crate) fn detach_desktop_visibility(&mut self, controller: u64) {
        self.visibility.detach(controller);
    }

    pub(crate) fn report_remote_visibility(&mut self, ids: HashSet<SessionId>) {
        self.visibility.remote(ids);
    }

    pub(crate) fn apply_visibility(&mut self) -> Result<()> {
        let eligible = self.visibility.eligible(self.owner.watched_sessions());
        self.owner
            .set_visible_sessions(eligible.as_deref())
            .context("Invalid activity polling interest")
    }

    pub fn new() -> Result<Self> {
        let instant = Instant::now();
        let unix_ms = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
        shellfox_host::activity_utc_timestamp(unix_ms)?;
        let clock = Arc::new(ActivityClock::new(instant, unix_ms));
        let (publisher, watch) = SourceFactsPublisher::new();
        Ok(Self {
            owner: OwnerActivity::with_clock(clock),
            persistence: ActivityPersistence::new(ActivityUtcAnchor {
                logical_ms: unix_ms,
                unix_ms,
            }),
            publisher,
            watch,
            rules: Vec::new(),
            next_poll_ms: unix_ms,
            dirty: true,
            visibility: Default::default(),
        })
    }
    pub fn now_ms(&self) -> Result<u64> {
        self.owner
            .clock()
            .now_ms()
            .context("Activity clock exceeded checked range.")
    }
}
