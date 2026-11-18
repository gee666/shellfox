//! Trusted owner receipts. No wire method or command/output persistence.
use super::*;
use shellfox_runtime::{Identity, SourceSequence};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutomationCleanupProof {
    run_id: Uuid,
    final_source_sequence: SourceSequence,
    receipt_id: Uuid,
    runtime_identity: Identity,
    kernel_exit_code: i64,
}
impl AutomationCleanupProof {
    pub fn run_id(self) -> Uuid {
        self.run_id
    }
    pub fn final_source_sequence(self) -> SourceSequence {
        self.final_source_sequence
    }
    pub fn runtime_identity(self) -> Identity {
        self.runtime_identity
    }
    pub fn kernel_exit_code(self) -> i64 {
        self.kernel_exit_code
    }
    /// Constructors are private. Only process-none, confirmed cleanup plus
    /// accepted ordered Exit can mint this receipt.
    pub fn raw_cleanup_confirmed(self) -> bool {
        true
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutomationPublication {
    pub session_id: Uuid,
    pub tab_id: Uuid,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutomationRetirement {
    proof: AutomationCleanupProof,
    publication: Option<AutomationPublication>,
}
impl AutomationRetirement {
    pub fn proof(self) -> AutomationCleanupProof {
        self.proof
    }
    pub fn publication(self) -> Option<AutomationPublication> {
        self.publication
    }
}
/// Service supplies the original canonical Runtime identity AFTER final feed.
/// Published parsers transfer to existing terminal ownership, not destruction.
pub enum AutomationRetirementAck {
    UnshownParserClosed { identity: Identity },
    PublishedParserTransferred { identity: Identity },
    PublishedParserClosed { identity: Identity },
}
impl AutomationRuns {
    /// Trusted Service calls this only after its matching relay confirmed ordered
    /// Finish. Explicit close retains this receipt across metadata commit retries.
    pub fn begin_retire(&mut self, id: Uuid) -> Result<AutomationRetirement> {
        if let Some(receipt) = self.retired.get(&id) {
            return Ok(*receipt);
        }
        self.retire_for_admission(id)?;
        self.retired
            .get(&id)
            .copied()
            .context("Automation retirement receipt disappeared.")
    }
    pub fn retired(&self) -> Vec<AutomationRetirement> {
        self.retired.values().copied().collect()
    }
    pub(super) fn retire_for_admission(&mut self, id: Uuid) -> Result<()> {
        ensure!(
            self.retired.len() < AUTOMATION_RUNS,
            "Automation parser retirement queue is full; no new process was launched."
        );
        let run = self
            .runs
            .get(&id)
            .context("Automation retirement record disappeared.")?;
        ensure!(
            run.process.is_none()
                && run.dto.state == AutomationState::Exited
                && run.eof == [true; 2]
                && run.pending.is_none()
                && run.frames.is_empty()
                && run.source.is_none()
                && run.source_failure.is_none()
                && run.failed_cleanup.is_none(),
            "Automation raw cleanup/final source publication is not confirmed."
        );
        ensure!(
            run.launch_evidence.is_none(),
            "Native launch receipt must transfer to the canonical Runtime before retirement."
        );
        let runtime_identity = run
            .source_identity
            .context("Canonical automation Runtime identity was not bound; record retained.")?;
        let publication = match (run.dto.session_id, run.dto.tab_id) {
            (Some(session_id), Some(tab_id)) => Some(AutomationPublication { session_id, tab_id }),
            (None, None) => None,
            _ => anyhow::bail!("Automation publication identity is incomplete."),
        };
        let receipt = AutomationRetirement {
            proof: AutomationCleanupProof {
                run_id: id,
                final_source_sequence: SourceSequence(run.sequence),
                receipt_id: Uuid::new_v4(),
                runtime_identity,
                kernel_exit_code: compatible_natural_exit(
                    run.outcome.context("Raw kernel exit status is missing.")?,
                ),
            },
            publication,
        };
        ensure!(
            !self.retired.contains_key(&id),
            "Automation retirement proof already exists."
        );
        self.retired.insert(id, receipt);
        self.runs.remove(&id);
        Ok(())
    }
    /// Caller must have proved relay.finished for the SAME immutable identity.
    /// Store the returned/captured proof with a published terminal so its later
    /// closure does not consult a reused/missing run ID in this registry.
    pub fn ack_retired(
        &mut self,
        proof: AutomationCleanupProof,
        ack: AutomationRetirementAck,
    ) -> Result<()> {
        let receipt = self
            .retired
            .get(&proof.run_id)
            .context("Automation retirement receipt not found.")?;
        ensure!(receipt.proof == proof, "Stale automation retirement proof.");
        let identity = match &ack {
            AutomationRetirementAck::UnshownParserClosed { identity }
            | AutomationRetirementAck::PublishedParserTransferred { identity }
            | AutomationRetirementAck::PublishedParserClosed { identity } => *identity,
        };
        ensure!(
            identity == receipt.proof.runtime_identity,
            "Automation retirement Runtime identity/generation changed."
        );
        match (receipt.publication, ack) {
            (None, AutomationRetirementAck::UnshownParserClosed { .. }) => {}
            (
                Some(publication),
                AutomationRetirementAck::PublishedParserTransferred { identity }
                | AutomationRetirementAck::PublishedParserClosed { identity },
            ) => {
                ensure!(
                    identity.session_id.to_string() == publication.session_id.to_string()
                        && identity.tab_id.to_string() == publication.tab_id.to_string(),
                    "Published automation Runtime identity changed."
                );
            }
            _ => anyhow::bail!(
                "Automation retirement disposition does not match publication ownership."
            ),
        }
        self.retired.remove(&proof.run_id);
        Ok(())
    }
}
