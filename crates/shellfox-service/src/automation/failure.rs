//! Failed delivery never becomes successful EOF/Finish. Raw cleanup is separate.
use super::*;
use shellfox_runtime::{Identity, Runtime, SourceSequence};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutomationSourceFailureKind {
    SourceOrder,
    FeedRejected,
    EofOrder,
    ExitOrder,
    FinishRejected,
    RuntimeClosed,
    ReceiverDropped,
    StartupCancelled,
    RawSpawnFailed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutomationSourceAbortReason {
    StartupCancelled,
    RawSpawnFailed,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AutomationSourceFailure {
    identity: Option<Identity>,
    last_queued: SourceSequence,
    pending: Option<SourceSequence>,
    pending_bytes: usize,
    kind: AutomationSourceFailureKind,
}
impl AutomationSourceFailure {
    pub(super) fn new(
        identity: Option<Identity>,
        last_queued: SourceSequence,
        pending: Option<SourceSequence>,
        pending_bytes: usize,
        kind: AutomationSourceFailureKind,
    ) -> Self {
        Self {
            identity,
            last_queued,
            pending,
            pending_bytes,
            kind,
        }
    }
    pub fn identity(self) -> Option<Identity> {
        self.identity
    }
    /// Accepted by the bounded producer queue, not necessarily parsed yet.
    pub fn last_queued(self) -> SourceSequence {
        self.last_queued
    }
    pub fn pending_sequence(self) -> Option<SourceSequence> {
        self.pending
    }
    pub fn pending_bytes(self) -> usize {
        self.pending_bytes
    }
    pub fn kind(self) -> AutomationSourceFailureKind {
        self.kind
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AutomationFailedCleanupProof {
    run_id: Uuid,
    failure: AutomationSourceFailure,
    exit_code: i64,
    receipt_id: Uuid,
}
impl AutomationFailedCleanupProof {
    pub fn run_id(self) -> Uuid {
        self.run_id
    }
    pub fn failure(self) -> AutomationSourceFailure {
        self.failure
    }
    pub fn exit_code(self) -> i64 {
        self.exit_code
    }
    pub fn raw_cleanup_confirmed(self) -> bool {
        true
    }
}
/// Opaque owned bytes from the failed delivery. Intentionally no Debug,
/// serialization or public byte accessor. Retain at most one per owned source.
pub struct AutomationFailedSourceEvidence {
    proof: AutomationFailedCleanupProof,
    pending: Option<AutomationViewEvent>,
}
impl AutomationFailedSourceEvidence {
    pub(super) fn new(
        proof: AutomationFailedCleanupProof,
        pending: Option<AutomationViewEvent>,
    ) -> Self {
        Self { proof, pending }
    }
    pub fn proof(&self) -> AutomationFailedCleanupProof {
        self.proof
    }
    pub fn pending_bytes(&self) -> usize {
        match &self.pending {
            Some(AutomationViewEvent::Data { bytes, .. }) => bytes.len(),
            _ => 0,
        }
    }
    #[cfg(all(test, windows))]
    pub(super) fn pending_for_test(&self) -> Option<&[u8]> {
        match &self.pending {
            Some(AutomationViewEvent::Data { bytes, .. }) => Some(bytes),
            _ => None,
        }
    }
}
impl AutomationRuns {
    pub fn begin_failed_source_cleanup(
        &mut self,
        id: Uuid,
        failure: AutomationSourceFailure,
    ) -> Result<()> {
        let run = self
            .runs
            .get_mut(&id)
            .context("Failed automation run not found.")?;
        ensure!(
            run.source_identity == failure.identity,
            "Failed automation source Runtime identity changed."
        );
        ensure!(
            run.source_failure
                .is_none_or(|original| original == failure),
            "Original automation source failure evidence changed."
        );
        run.source_failure = Some(failure);
        run.stopping = true;
        run.release();
        if let Some(process) = &mut run.process {
            process.request_stop()?;
        }
        Ok(())
    }
    pub fn failed_source_cleanup(&self, id: Uuid) -> Option<AutomationFailedCleanupProof> {
        self.runs.get(&id).and_then(|run| run.failed_cleanup)
    }
}
impl Run {
    pub(super) fn poll_failed_cleanup(&mut self) -> Result<()> {
        let Some(failure) = self.source_failure else {
            return Ok(());
        };
        if self.failed_cleanup.is_some() {
            return Ok(());
        }
        let code = if let Some(process) = &mut self.process {
            let Some(code) = process.poll_exit()? else {
                return Ok(());
            };
            self.forced_stop = process.was_forcibly_stopped();
            process.close()?;
            code
        } else {
            self.outcome
                .context("Failed source lacks actual raw cleanup outcome.")?
        };
        self.process = None;
        self.outcome = Some(code);
        self.finalize_dto(code);
        // Keep pending event/queue/evidence. No synthetic EOFs or Exit are sent.
        self.failed_cleanup = Some(AutomationFailedCleanupProof {
            run_id: self.dto.id,
            failure,
            exit_code: compatible_natural_exit(code),
            receipt_id: Uuid::new_v4(),
        });
        Ok(())
    }
    pub(super) fn dropped_source_failure(&self) -> AutomationSourceFailure {
        let (pending, pending_bytes) = match &self.pending {
            Some(AutomationViewEvent::Data {
                sequence, bytes, ..
            }) => (Some(SourceSequence(*sequence)), bytes.len()),
            Some(
                AutomationViewEvent::Eof { after_sequence, .. }
                | AutomationViewEvent::Exit { after_sequence, .. },
            ) => (Some(SourceSequence(*after_sequence)), 0),
            None => (None, 0),
        };
        AutomationSourceFailure::new(
            self.source_identity,
            SourceSequence(self.sequence),
            pending,
            pending_bytes,
            AutomationSourceFailureKind::ReceiverDropped,
        )
    }
}
impl AutomationRelay {
    /// Observe actual canonical owner failure even with no new source data.
    /// This records failure, never an EOF or proof that the raw process exited.
    pub fn observe_runtime(&mut self, runtime: &Runtime) -> Result<()> {
        ensure!(
            runtime.identity() == self.identity(),
            "Automation canonical Runtime identity changed."
        );
        if runtime
            .external_status()
            .is_some_and(|status| status.failed || (status.closed && !status.finished))
        {
            self.record_failure(AutomationSourceFailureKind::RuntimeClosed);
            anyhow::bail!(
                "Automation canonical parser failed/closed; raw cleanup remains required."
            );
        }
        Ok(())
    }
}
