use super::{
    inspection::validate, matching, Health, InspectionFailure, Observation, OwnedProcessSnapshot,
    ProcessIdentity, ProcessInspector, ProcessPlatform, RootState,
};
use shellfox_core::SettingsProcessRule;
use shellfox_runtime::Identity;
use std::collections::HashSet;

pub const PROCESS_POLL_MS: u64 = 1000;
pub const MAX_WATCHES: usize = 1000;
pub const MAX_CACHED_PROCESS_IDENTITIES: usize = 20_000;
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessUpdate {
    pub identity: Identity,
    pub observation: Observation,
    /// Health/count or exact descendant identity changed, not merely poll time.
    pub activity_changed: bool,
    /// A changed authoritative live/root-exit observation may refresh TTL.
    /// Diagnostic Unavailable/Unsupported transitions never do.
    pub touch_session: bool,
}
struct Watch {
    identity: Identity,
    expected_shell: String,
    root: Option<ProcessIdentity>,
    next_poll: u64,
    observation: Option<Observation>,
    activity_key: Option<(RootState, Health, u32)>,
    descendants: Vec<ProcessIdentity>,
}
#[derive(Default)]
pub struct ActivityMonitor {
    watches: Vec<Watch>,
    next: usize,
    visible: Option<HashSet<shellfox_core::SessionId>>,
}
impl ActivityMonitor {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn watch(
        &mut self,
        identity: Identity,
        expected_shell: String,
    ) -> Result<(), InspectionFailure> {
        if expected_shell.is_empty()
            || expected_shell.len() > super::inspection::MAX_FIELD
            || expected_shell.contains('\0')
        {
            return Err(InspectionFailure::Malformed);
        }
        if let Some(watch) = self
            .watches
            .iter_mut()
            .find(|watch| watch.identity.tab_id == identity.tab_id)
        {
            if watch.identity == identity && watch.expected_shell == expected_shell {
                return Ok(());
            }
            *watch = Watch::new(identity, expected_shell);
        } else {
            if self.watches.len() >= MAX_WATCHES {
                return Err(InspectionFailure::Bounds);
            }
            self.watches.push(Watch::new(identity, expected_shell));
        }
        Ok(())
    }
    pub fn forget(&mut self, identity: Identity) {
        self.watches.retain(|watch| watch.identity != identity);
        if self.watches.is_empty() {
            self.next = 0;
        } else {
            self.next %= self.watches.len();
        }
    }
    pub fn set_visible_sessions(
        &mut self,
        sessions: Option<&[shellfox_core::SessionId]>,
    ) -> Result<(), InspectionFailure> {
        if sessions.is_some_and(|sessions| sessions.len() > MAX_WATCHES) {
            return Err(InspectionFailure::Bounds);
        }
        let visible = sessions.map(|sessions| sessions.iter().copied().collect::<HashSet<_>>());
        if self.visible != visible {
            self.visible = visible;
            for watch in &mut self.watches {
                watch.next_poll = 0;
            }
        }
        Ok(())
    }
    /// At most 16 owned inspections per tick. No work is started without a watch.
    pub fn poll(
        &mut self,
        inspector: &mut impl ProcessInspector,
        rules: &[SettingsProcessRule],
        now_ms: u64,
        budget: usize,
    ) -> Vec<ProcessUpdate> {
        self.poll_scope(inspector, rules, now_ms, budget, false)
    }
    /// Explicit expiry/cleanup refresh includes hidden roots and ignores the timer.
    /// The caller repeats bounded batches until every required generation is fresh.
    pub fn poll_all(
        &mut self,
        inspector: &mut impl ProcessInspector,
        rules: &[SettingsProcessRule],
        now_ms: u64,
        budget: usize,
    ) -> Vec<ProcessUpdate> {
        self.poll_scope(inspector, rules, now_ms, budget, true)
    }
    fn poll_scope(
        &mut self,
        inspector: &mut impl ProcessInspector,
        rules: &[SettingsProcessRule],
        now_ms: u64,
        budget: usize,
        force: bool,
    ) -> Vec<ProcessUpdate> {
        let count = self.watches.len();
        let mut updates = Vec::new();
        let mut attempted = 0;
        if count == 0 {
            return updates;
        }
        let rules_valid = rules.len() <= 100 && rules.iter().all(|rule| rule.validate().is_ok());
        let start = self.next % count;
        for offset in 0..count {
            if attempted >= budget.min(16) {
                break;
            }
            let index = (start + offset) % count;
            let other_cached = self
                .watches
                .iter()
                .map(|watch| watch.descendants.len())
                .sum::<usize>()
                - self.watches[index].descendants.len();
            let watch = &mut self.watches[index];
            if !force
                && (now_ms < watch.next_poll
                    || self
                        .visible
                        .as_ref()
                        .is_some_and(|sessions| !sessions.contains(&watch.identity.session_id)))
            {
                continue;
            }
            attempted += 1;
            watch.next_poll = now_ms.saturating_add(PROCESS_POLL_MS);
            let result = if rules_valid {
                inspector
                    .inspect(watch.identity)
                    .and_then(|snapshot| watch.observe(snapshot, rules, now_ms))
            } else {
                Err(InspectionFailure::Malformed)
            };
            if matches!(&result, Err(InspectionFailure::Pending)) {
                watch.next_poll = now_ms;
                self.next = (index + 1) % count;
                continue;
            }
            let (mut observation, descendants) = result.unwrap_or_else(|error| {
                (
                    Observation {
                        root: RootState::Unavailable,
                        health: Health::Unknown,
                        agents: 0,
                        reason: Some(error.to_string()),
                        observed_at_ms: now_ms,
                    },
                    Vec::new(),
                )
            });
            let descendants = if other_cached + descendants.len() > MAX_CACHED_PROCESS_IDENTITIES {
                // Keep proven identities on cache pressure, but never claim fresh health.
                observation = Observation {
                    root: RootState::Unavailable,
                    health: Health::Unknown,
                    agents: 0,
                    reason: Some(InspectionFailure::Bounds.to_string()),
                    observed_at_ms: now_ms,
                };
                None
            } else {
                Some(descendants)
            };
            let activity_key = (observation.root, observation.health, observation.agents);
            let changed = watch.activity_key != Some(activity_key)
                || descendants
                    .as_ref()
                    .is_some_and(|new| watch.descendants != *new);
            watch.activity_key = Some(activity_key);
            watch.observation = Some(observation.clone());
            if let Some(descendants) = descendants {
                watch.descendants = descendants;
            }
            let touch_session =
                changed && matches!(observation.root, RootState::Alive | RootState::Exited);
            updates.push(ProcessUpdate {
                identity: watch.identity,
                observation,
                activity_changed: changed,
                touch_session,
            });
            self.next = (index + 1) % count;
        }
        updates
    }
    pub fn observation(&self, identity: Identity) -> Option<&Observation> {
        self.watches
            .iter()
            .find(|watch| watch.identity == identity)
            .and_then(|watch| watch.observation.as_ref())
    }
    /// Expiry proof covers every currently owned generation, not runtime Phase::Active.
    pub fn fresh_healthy(&self, identities: &[Identity], since_ms: u64) -> bool {
        identities.len() <= MAX_WATCHES
            && identities.iter().all(|identity| {
                self.observation(*identity).is_some_and(|observation| {
                    observation.root == RootState::Alive
                        && observation.health == Health::Healthy
                        && observation.observed_at_ms >= since_ms
                })
            })
    }
    /// Settings changes invalidate facts without discarding authenticated root identity.
    pub fn rules_changed(&mut self) {
        for watch in &mut self.watches {
            watch.observation = None;
            watch.next_poll = 0;
        }
    }
}
impl Watch {
    fn new(identity: Identity, expected_shell: String) -> Self {
        Self {
            identity,
            expected_shell,
            root: None,
            next_poll: 0,
            observation: None,
            activity_key: None,
            descendants: Vec::new(),
        }
    }
    fn observe(
        &mut self,
        snapshot: OwnedProcessSnapshot,
        rules: &[SettingsProcessRule],
        now_ms: u64,
    ) -> Result<(Observation, Vec<ProcessIdentity>), InspectionFailure> {
        if snapshot.identity != self.identity {
            return Err(InspectionFailure::OwnershipMismatch);
        }
        validate(&snapshot)?;
        let windows = snapshot.platform == ProcessPlatform::Windows;
        let mut root = snapshot.root;
        let root_row = snapshot.processes.iter().find(|process| process.is_root);
        let mut reason = (!snapshot.complete).then_some("Process enumeration was incomplete.");
        if root == RootState::Alive {
            if let Some(row) = root_row.filter(|row| row.accessible && row.executable.is_some()) {
                let expected = matching::basename(&self.expected_shell, windows);
                let same = |name: &str| {
                    let actual = matching::basename(name, windows);
                    if windows {
                        actual.to_lowercase() == expected.to_lowercase()
                    } else {
                        actual == expected
                    }
                };
                let executable = row.executable.as_deref().unwrap_or_default();
                // macOS /bin/sh is a shim that execs bash and keeps argv[0] = sh.
                let sh_shim = cfg!(target_os = "macos")
                    && expected == "sh"
                    && matching::basename(executable, false) == "bash"
                    && row
                        .argv
                        .as_ref()
                        .and_then(|argv| argv.first())
                        .is_some_and(|first| same(first.trim_start_matches('-')));
                let same_shell = same(executable) || sh_shim;
                if !same_shell
                    || self
                        .root
                        .as_ref()
                        .is_some_and(|pinned| *pinned != row.identity)
                {
                    root = RootState::Unavailable;
                    reason = Some("Owned shell executable or exact birth changed.");
                } else {
                    self.root = Some(row.identity.clone());
                }
            } else {
                root = RootState::Unavailable;
                reason = Some("Owned shell identity or access could not be verified.");
            }
        }
        let mut agents = 0;
        let mut descendants = Vec::new();
        for process in snapshot.processes.iter().filter(|process| !process.is_root) {
            descendants.push(process.identity.clone());
            if !process.accessible || process.executable.is_none() {
                reason = Some("A descendant is inaccessible.");
                continue;
            }
            let (matched, failure) = matching::match_agent(process, rules, windows);
            if failure.is_some() {
                reason = failure;
            }
            if matched {
                agents += 1;
            }
        }
        descendants.sort();
        let observation = Observation {
            root,
            health: if root == RootState::Alive && reason.is_none() {
                Health::Healthy
            } else {
                Health::Unknown
            },
            agents: if root == RootState::Alive { agents } else { 0 },
            reason: reason.map(str::to_owned),
            observed_at_ms: now_ms,
        };
        Ok((observation, descendants))
    }
}
