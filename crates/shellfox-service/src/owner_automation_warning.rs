use super::*;
use crate::automation::AutomationFailedCleanupProof;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PresentationShutdownState {
    NoKnownFailure,
    FailedSources(PresentationWarnings),
}
#[derive(Clone)]
pub struct PresentationWarnings {
    failures: Arc<[AutomationFailedCleanupProof]>,
    _evidence: Arc<OpaqueEvidence>,
}
// Neither raw commands/environment nor source bytes can enter Debug/serde.
struct OpaqueEvidence {
    _runs: AutomationRuns,
    _relays: Vec<Relay>,
}
impl std::fmt::Debug for PresentationWarnings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresentationWarnings")
            .field("failed_sources", &self.failures.len())
            .finish_non_exhaustive()
    }
}
impl PartialEq for PresentationWarnings {
    fn eq(&self, other: &Self) -> bool {
        self.failures == other.failures
    }
}
impl Eq for PresentationWarnings {}
impl PresentationWarnings {
    pub fn failures(&self) -> &[AutomationFailedCleanupProof] {
        &self.failures
    }
}
pub struct AutomationViewFailure {
    pub proof: AutomationFailedCleanupProof,
}
impl std::fmt::Debug for AutomationViewFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutomationViewFailure")
            .field("raw_cleanup", &"confirmed")
            .field("final_presentation", &"failed")
            .finish_non_exhaustive()
    }
}
impl std::fmt::Display for AutomationViewFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Original raw process cleanup is confirmed, but final automation rendering failed; exact source evidence is retained.")
    }
}
impl std::error::Error for AutomationViewFailure {}
impl Service {
    pub(crate) fn presentation_shutdown_state(&mut self) -> PresentationShutdownState {
        if self.automation.presentation_warning.is_none()
            && !self.automation.failed_evidence.is_empty()
        {
            let failed = std::mem::take(&mut self.automation.failed_evidence);
            let failures = failed
                .iter()
                .map(|failed| failed.proof)
                .collect::<Vec<_>>()
                .into();
            let relays = failed.into_iter().map(|failed| failed.relay).collect();
            let runs = std::mem::replace(
                &mut self.automation.runs,
                AutomationRuns::new(self.supervisor.clone()),
            );
            self.automation.presentation_warning = Some(PresentationWarnings {
                failures,
                _evidence: Arc::new(OpaqueEvidence {
                    _runs: runs,
                    _relays: relays,
                }),
            });
        }
        self.automation
            .presentation_warning
            .clone()
            .map(PresentationShutdownState::FailedSources)
            .unwrap_or(PresentationShutdownState::NoKnownFailure)
    }
    pub(super) fn close_failed_automation_sources(&mut self) -> Result<()> {
        let failed: Vec<_> = self
            .automation
            .relays
            .iter()
            .filter_map(|(id, relay)| {
                self.automation
                    .runs
                    .failed_source_cleanup(*id)
                    .map(|proof| (*id, relay.feed.identity(), proof))
            })
            .collect();
        for (id, identity, proof) in failed {
            if !proof.raw_cleanup_confirmed() || proof.failure().identity() != Some(identity) {
                bail!("Failed automation raw cleanup proof does not match canonical identity.");
            }
            if let Some(relay) = self.automation.relays.get(&id) {
                let _ = relay.activity.raw_exit_observed(Instant::now());
            }
            self.collect_automation_exit_touches();
            let terminal = self
                .terminals
                .get(&identity.tab_id)
                .context("Failed canonical parser owner is missing.")?;
            if terminal.runtime.identity() != identity {
                bail!("Failed canonical parser generation changed.");
            }
            if terminal
                .runtime
                .external_status()
                .is_some_and(|status| status.finished || status.failed)
            {
                terminal.runtime.request_external_close()?;
            } else {
                // The sole sender was aborted. Accepted FIFO data must reach the
                // parser before its real disconnect/failure; do not bypass the
                // unfinished-close guard or invent a successful Finish.
                continue;
            }
            if self
                .terminals
                .get_mut(&identity.tab_id)
                .context("Failed canonical parser owner disappeared.")?
                .runtime
                .poll_external_closed()?
            {
                self.activity_collect_live();
                self.forget_activity(identity);
                self.terminals.remove(&identity.tab_id);
                self.ownership_source_destroyed(identity);
                let relay = self
                    .automation
                    .relays
                    .remove(&id)
                    .context("Failed canonical source evidence disappeared.")?;
                self.automation
                    .failed_evidence
                    .push(FailedEvidence { proof, relay });
            }
        }
        Ok(())
    }
}
