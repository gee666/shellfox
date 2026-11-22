//! One raw owner, one reserved passive parser, and bounded deferred replies.
#[path = "owner_automation_close.rs"]
mod close;
#[path = "owner_automation_activity.rs"]
mod live_activity;
use live_activity::AutomationActivity;
#[path = "owner_automation_dispatch.rs"]
mod dispatch;
#[path = "owner_automation_relay.rs"]
mod relay;
#[path = "owner_automation_retirement.rs"]
mod retirement;
#[path = "owner_automation_show.rs"]
mod show;
#[path = "owner_automation_start.rs"]
mod start;
#[cfg(test)]
#[path = "owner_automation_test_bridge.rs"]
mod test_bridge;
#[cfg(test)]
#[path = "owner_automation_tests.rs"]
mod tests;
#[path = "owner_automation_warning.rs"]
mod warning;
use crate::automation::{AutomationRelay, AutomationRuns};
use crate::*;
use shellfox_protocol::automation::*;
use shellfox_runtime::{Identity, SourceKind};
use shellfox_terminal::raw_process::WriteReceipt;
pub use show::{AUTOMATION_PROFILE_ID, AUTOMATION_READONLY_PROFILE};
use tokio::sync::oneshot;
use uuid::Uuid;
pub use warning::{AutomationViewFailure, PresentationShutdownState, PresentationWarnings};

type JsonReply = oneshot::Sender<Result<serde_json::Value>>;
// Leave connection slots for output/metadata while actual stdin writes wait.
const MAX_DEFERRED: usize = 8;
pub(crate) struct AutomationState {
    runs: AutomationRuns,
    relays: HashMap<Uuid, Relay>,
    writes: Vec<PendingWrite>,
    stops: Vec<PendingStop>,
    fault: Option<String>,
    startup: Vec<PendingStartup>,
    requested_stops: std::collections::HashSet<Uuid>,
    failed_evidence: Vec<FailedEvidence>,
    presentation_warning: Option<PresentationWarnings>,
    closed_external: HashMap<TabId, ClosedExternal>,
}
impl AutomationState {
    /// Raw automation processes and their relays are pumped, not event-driven.
    pub(crate) fn busy(&self) -> bool {
        !self.relays.is_empty()
            || !self.startup.is_empty()
            || !self.writes.is_empty()
            || !self.stops.is_empty()
            || self.runs.active_processes() > 0
    }
}
struct Relay {
    feed: AutomationRelay,
    failed_start: bool,
    activity: std::sync::Arc<AutomationActivity>,
}
enum ClosedExternal {
    Closing(crate::automation::AutomationRetirement),
    Joined(crate::automation::AutomationRetirement),
    Acknowledged(crate::automation::AutomationCleanupProof),
}
struct FailedEvidence {
    proof: crate::automation::AutomationFailedCleanupProof,
    relay: Relay,
}
struct PendingStartup {
    id: Uuid,
    controller: u64,
    owner: shellfox_runtime::ExternalStartupOwner,
}
struct PendingWrite {
    receipt: WriteReceipt,
    reply: JsonReply,
}
struct PendingStop {
    id: Uuid,
    reply: JsonReply,
}
impl AutomationState {
    pub fn new(supervisor: PathBuf) -> Self {
        Self {
            runs: AutomationRuns::new(supervisor),
            relays: HashMap::new(),
            writes: vec![],
            stops: vec![],
            fault: None,
            startup: vec![],
            requested_stops: std::collections::HashSet::new(),
            failed_evidence: vec![],
            presentation_warning: None,
            closed_external: HashMap::new(),
        }
    }
}
impl Service {
    /// Completed screen evidence needs both raw exit and the original parser's
    /// ordered Finish. This does not confer remote delivery eligibility.
    pub(crate) fn completed_automation_exit_code(
        &self,
        terminal: &terminals::OwnedTerminal,
    ) -> Option<i64> {
        let runtime = &terminal.runtime;
        let identity = runtime.identity();
        if let Some(proof) = terminal.external_cleanup {
            let status = runtime.external_status()?;
            return (proof.runtime_identity() == identity
                && status.finished
                && !status.closed
                && !status.failed
                && status.source_sequence == proof.final_source_sequence())
                .then(|| proof.kernel_exit_code());
        }
        let (id, relay) = self
            .automation
            .relays
            .iter()
            .find(|(_, relay)| !relay.failed_start && relay.feed.identity() == identity)?;
        if !relay.feed.finished(runtime) {
            return None;
        }
        self.automation.runs.completed_view_exit_code(*id, identity)
    }

    pub(super) fn begin_automation_stop(&mut self, request: AutomationStop) -> Result<()> {
        let id = request.id;
        self.automation.runs.begin_stop(request)?;
        self.automation.requested_stops.insert(id);
        Ok(())
    }
    pub(crate) fn release_automation_controller(&mut self, controller: u64) {
        self.automation.runs.release_owner(controller);
    }
    pub(crate) fn pump_automation(&mut self) {
        if let Err(error) = self.automation.pump(&self.terminals, self.stopping) {
            self.automation.fault = Some(error.to_string());
        }
        self.collect_automation_exit_touches();
        if let Err(error) = self
            .pump_automation_retirement()
            .and_then(|()| self.retire_failed_automation())
        {
            self.automation.fault = Some(error.to_string());
        }
    }
    pub(super) fn collect_automation_exit_touches(&mut self) {
        for relay in self.automation.relays.values() {
            if let Some(at) = relay.activity.take_exit_touch() {
                let identity = relay.feed.identity();
                let _ = self.activity.owner.confirmed_exit(identity);
                if let Some(now) = self.activity.owner.clock().at(at) {
                    let _ = self.activity.persistence.touch(identity.session_id, now);
                    self.activity.dirty = true;
                }
            }
        }
    }
    pub(crate) fn stop_automation(&mut self) -> Result<()> {
        self.automation.runs.begin_shutdown()?;
        self.wait_until(
            Instant::now() + Duration::from_secs(12),
            |service| {
                service.pump();
                service.retire_failed_automation()?;
                service.close_failed_automation_sources()?;
                let finished = service.automation.startup.is_empty()
                    && service.automation.runs.shutdown_ready()
                    && service.automation.relays.values().all(|relay| {
                        service.terminals
                            .get(&relay.feed.identity().tab_id)
                            .is_some_and(|terminal| relay.feed.finished(&terminal.runtime))
                    });
                Ok(finished.then_some(()))
            },
            || anyhow::anyhow!("Automation cleanup/final source drain is unconfirmed; raw and parser owners retained."),
        )
    }
    fn retire_failed_automation(&mut self) -> Result<()> {
        let failed: Vec<_> = self
            .automation
            .relays
            .iter()
            .filter(|(id, relay)| {
                relay.failed_start
                    && !self.automation.runs.cleanup_pending(**id)
                    && self
                        .terminals
                        .get(&relay.feed.identity().tab_id)
                        .and_then(|terminal| terminal.runtime.external_status())
                        .is_some_and(|status| status.failed)
            })
            .map(|(_, relay)| relay.feed.identity())
            .collect();
        for identity in failed {
            // The raw launch never returned READY and its retained original
            // owner is now confirmed gone. No source bytes were accepted.
            self.stop_terminal(TerminalIdentity {
                tab_id: identity.tab_id,
                generation: identity.generation,
            })?;
        }
        Ok(())
    }
}
