use super::{
    ActivityClock, InspectionScope, LiveActivitySink, LiveActivityUpdate, RuntimeInspectionState,
    RuntimeProcessInspector, RuntimeSource, TerminalSourceFacts,
};
use crate::activity::*;
use shellfox_core::{SettingsProcessRule, TabId};
use shellfox_runtime::{Identity, LiveOutputStatus, Phase};
use std::{collections::HashMap, sync::Arc, time::Instant};
#[cfg(test)]
#[path = "activity_integration_owner_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InputActivity {
    pub touch_session: bool,
    pub unsettle: bool,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OutputActivity {
    pub touch_session: bool,
    pub busy_changed: Option<bool>,
}
struct TerminalPolicy {
    identity: Identity,
    scope: InspectionScope,
    exit_seen: bool,
    activity: Arc<LiveActivitySink>,
}
pub struct OwnerActivity {
    clock: Arc<ActivityClock>,
    monitor: ActivityMonitor,
    inspections: RuntimeInspectionState,
    terminals: HashMap<TabId, TerminalPolicy>,
}
impl Default for OwnerActivity {
    fn default() -> Self {
        Self::with_clock(Arc::new(ActivityClock::new(Instant::now(), 0)))
    }
}
impl OwnerActivity {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_clock(clock: Arc<ActivityClock>) -> Self {
        Self {
            clock,
            monitor: ActivityMonitor::new(),
            inspections: RuntimeInspectionState::new(),
            terminals: HashMap::new(),
        }
    }
    pub fn clock(&self) -> &Arc<ActivityClock> {
        &self.clock
    }
    pub fn watch(
        &mut self,
        identity: Identity,
        expected_shell: String,
        scope: InspectionScope,
    ) -> Result<(), InspectionFailure> {
        if !self.terminals.contains_key(&identity.tab_id) && self.terminals.len() >= MAX_WATCHES {
            return Err(InspectionFailure::Bounds);
        }
        self.monitor.watch(identity, expected_shell)?;
        self.inspections.watch(identity, scope)?;
        if self
            .terminals
            .get(&identity.tab_id)
            .is_none_or(|entry| entry.identity != identity)
        {
            self.terminals.insert(
                identity.tab_id,
                TerminalPolicy {
                    identity,
                    scope,
                    exit_seen: false,
                    activity: Arc::new(LiveActivitySink::new(identity, Arc::clone(&self.clock))),
                },
            );
        }
        if let Some(entry) = self.terminals.get_mut(&identity.tab_id) {
            entry.scope = scope;
        }
        Ok(())
    }
    /// Install this exact shared object in RuntimeHooks BEFORE starting the PTY.
    pub fn live_output_sink(&self, identity: Identity) -> Option<Arc<LiveActivitySink>> {
        self.terminals
            .get(&identity.tab_id)
            .filter(|entry| entry.identity == identity)
            .map(|entry| Arc::clone(&entry.activity))
    }
    /// Never turn later persistence failure into a failed write that a client retries.
    pub fn accepted_input(
        &self,
        identity: Identity,
        bytes: &[u8],
        user_initiated: bool,
        current_settled: Option<&str>,
        captured_settled: Option<Option<&str>>,
    ) -> InputActivity {
        let touch_session = self
            .terminals
            .get(&identity.tab_id)
            .is_some_and(|entry| entry.identity == identity && !entry.exit_seen)
            && meaningful_input(bytes, user_initiated);
        InputActivity {
            touch_session,
            unsettle: touch_session && should_unsettle(current_settled, captured_settled),
        }
    }
    /// No Event::Output feed here: it is view credit, not the authoritative stream.
    pub fn take_live_updates(&self) -> Vec<LiveActivityUpdate> {
        self.terminals
            .values()
            .filter_map(|entry| entry.activity.take_update())
            .collect()
    }
    /// Check this even while process polling is paused/hidden. Failure affects
    /// monitoring only, never the PTY, and cannot authorize inactivity deletion.
    pub fn sync_live_status(&self, source: &dyn RuntimeSource) {
        for entry in self.terminals.values() {
            if source
                .runtime(entry.identity)
                .is_some_and(|runtime| runtime.live_output_status() != LiveOutputStatus::Active)
            {
                entry.activity.disable();
            }
        }
    }
    /// Only after authoritative final exit; repeated polling must not touch TTL.
    pub fn confirmed_exit(&mut self, identity: Identity) -> OutputActivity {
        let Some(entry) = self
            .terminals
            .get_mut(&identity.tab_id)
            .filter(|entry| entry.identity == identity)
        else {
            return OutputActivity::default();
        };
        let touch_session = !entry.exit_seen;
        entry.exit_seen = true;
        entry.activity.cancel_all_inputs(identity);
        OutputActivity {
            touch_session,
            busy_changed: entry.activity.stop().ok().flatten(),
        }
    }
    pub fn tick(&self, now_ms: u64) -> Vec<(Identity, bool)> {
        self.terminals
            .values()
            .filter_map(|entry| {
                entry
                    .activity
                    .tick(now_ms)
                    .ok()
                    .flatten()
                    .map(|busy| (entry.identity, busy))
            })
            .collect()
    }
    /// Visibility does not forget identities or in-flight inspection tickets.
    pub fn forget(&mut self, identity: Identity) -> Option<bool> {
        self.monitor.forget(identity);
        self.inspections.forget(identity);
        if self
            .terminals
            .get(&identity.tab_id)
            .is_some_and(|entry| entry.identity == identity)
        {
            self.terminals.remove(&identity.tab_id).and_then(|entry| {
                entry.activity.cancel_all_inputs(identity);
                entry.activity.stop().ok().flatten()
            })
        } else {
            None
        }
    }
    pub fn set_visible_sessions(
        &mut self,
        sessions: Option<&[shellfox_core::SessionId]>,
    ) -> Result<(), InspectionFailure> {
        self.monitor.set_visible_sessions(sessions)
    }
    pub fn rules_changed(&mut self) {
        self.monitor.rules_changed();
    }
    pub fn poll_processes(
        &mut self,
        source: &dyn RuntimeSource,
        rules: &[SettingsProcessRule],
        now_ms: u64,
        budget: usize,
        refresh_all: bool,
    ) -> Vec<ProcessUpdate> {
        self.sync_live_status(source);
        let mut inspector = RuntimeProcessInspector::new(source, &mut self.inspections, now_ms);
        let mut updates = if refresh_all {
            self.monitor.poll_all(&mut inspector, rules, now_ms, budget)
        } else {
            self.monitor.poll(&mut inspector, rules, now_ms, budget)
        };
        self.apply_exit_barrier(&mut updates);
        updates
    }
    fn apply_exit_barrier(&self, updates: &mut [ProcessUpdate]) {
        for update in updates {
            update.touch_session &= self
                .terminals
                .get(&update.identity.tab_id)
                .is_some_and(|entry| entry.identity == update.identity && !entry.exit_seen);
        }
    }
    pub fn begin_expiry_refresh(&mut self, identities: &[Identity], since_ms: u64) {
        self.inspections.require_fresh(identities, since_ms);
    }
    pub fn fresh_healthy(&self, identities: &[Identity], since_ms: u64) -> bool {
        self.inspections.fresh_since(identities, since_ms)
            && self.monitor.fresh_healthy(identities, since_ms)
            && identities.iter().all(|identity| {
                self.live_output_sink(*identity)
                    .is_some_and(|sink| sink.status().1)
            })
    }
    /// Capture only on the owner after sync_live_status; no Runtime/Host escapes.
    pub fn terminal_facts(&self, identity: Identity, phase: Phase) -> TerminalSourceFacts {
        let entry = self
            .terminals
            .get(&identity.tab_id)
            .filter(|entry| entry.identity == identity);
        let (busy, activity_available) =
            entry.map_or((false, false), |entry| entry.activity.status());
        let mut observation = entry
            .filter(|entry| entry.scope == InspectionScope::LocalWindows)
            .and_then(|_| self.monitor.observation(identity))
            .cloned();
        if !activity_available {
            if let Some(observation) = &mut observation {
                observation.health = Health::Unknown;
                observation.reason = Some("Output activity monitoring is unavailable.".into());
            }
        }
        TerminalSourceFacts {
            identity,
            phase,
            source_kind: None,
            startup: super::StartupEvidence::Pending,
            observation,
            busy,
            activity_available,
        }
    }
}
