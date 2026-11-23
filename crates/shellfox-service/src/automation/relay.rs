//! Owner-loop bridge to the ONE external Runtime. No process control here.
#[cfg(all(test, windows))]
mod tests;
use super::{
    AutomationFailedCleanupProof, AutomationFailedSourceEvidence, AutomationSourceAbortReason,
    AutomationSourceFailure, AutomationSourceFailureKind, AutomationView, AutomationViewEvent,
};
use anyhow::{ensure, Context, Result};
use shellfox_protocol::automation::AutomationStream;
use shellfox_runtime::{
    ExternalAbortEvidence, ExternalAbortReason, ExternalChunk, ExternalFeedError,
    ExternalFinishError, ExternalSource, ExternalStream, Identity, Runtime, SourceKind,
    SourceSequence,
};

pub struct AutomationRelay {
    identity: Identity,
    input: AutomationView,
    output: Option<ExternalSource>,
    pending: Option<AutomationViewEvent>,
    sequence: SourceSequence,
    eof: [bool; 2],
    finish_queued: bool,
    failure: Option<AutomationSourceFailure>,
    evidence_taken: bool,
    abort_intent: Option<ExternalAbortEvidence>,
}
impl AutomationRelay {
    /// Construct after Runtime::start_external returned ready, before raw launch.
    pub fn new(input: AutomationView, runtime: &mut Runtime) -> Result<Self> {
        ensure!(
            runtime.source_kind() == SourceKind::ExternalBytes,
            "Automation requires an external-byte Runtime, never a PTY."
        );
        let output = runtime
            .take_source()
            .context("Automation Runtime source was already taken.")?;
        Ok(Self {
            identity: runtime.identity(),
            input,
            output: Some(output),
            pending: None,
            sequence: SourceSequence(0),
            eof: [false; 2],
            finish_queued: false,
            failure: None,
            evidence_taken: false,
            abort_intent: None,
        })
    }
    /// Correlation capability from this sole ExternalSource, never a new owner.
    pub fn take_launch_binding(&mut self) -> Option<shellfox_terminal::NativeLaunchBinding> {
        self.output
            .as_mut()
            .and_then(ExternalSource::take_launch_binding)
    }
    pub fn bind_launch_evidence(
        &mut self,
        evidence: shellfox_terminal::NativeLaunchEvidence,
    ) -> Result<()> {
        self.output
            .as_mut()
            .context("Automation source owner is unavailable.")?
            .bind_launch_evidence(evidence)?;
        Ok(())
    }
    /// Bounded per tick. Full keeps the exact allocation, stream and sequence.
    pub fn pump(&mut self) -> Result<()> {
        if self.failure.is_some() {
            anyhow::bail!("Automation source has retained failure evidence; explicit raw cleanup is required.");
        }
        if self.finish_queued {
            return Ok(());
        }
        for _ in 0..64 {
            let Some(event) = self.pending.take().or_else(|| self.input.try_recv()) else {
                break;
            };
            match event {
                AutomationViewEvent::Data {
                    sequence,
                    stream,
                    bytes,
                } => {
                    let descriptor = usize::from(stream == AutomationStream::Stderr);
                    if self.eof[descriptor] || self.sequence.0.checked_add(1) != Some(sequence) {
                        self.pending = Some(AutomationViewEvent::Data {
                            sequence,
                            stream,
                            bytes,
                        });
                        self.record_failure(AutomationSourceFailureKind::SourceOrder);
                        anyhow::bail!(
                            "Automation source order changed; exact pending chunk retained."
                        );
                    }
                    let chunk = ExternalChunk {
                        sequence: SourceSequence(sequence),
                        stream: external_stream(stream),
                        bytes,
                    };
                    let sent = match self.output.as_mut() {
                        Some(output) => output.try_feed(chunk),
                        None => Err(ExternalFeedError::Closed(chunk)),
                    };
                    match sent {
                        Ok(()) => self.sequence = SourceSequence(sequence),
                        Err(error) => {
                            let full = matches!(error, ExternalFeedError::Full(_));
                            let chunk = match error {
                                ExternalFeedError::Full(chunk)
                                | ExternalFeedError::Closed(chunk)
                                | ExternalFeedError::Invalid { chunk, .. } => chunk,
                            };
                            self.pending = Some(AutomationViewEvent::Data {
                                sequence: chunk.sequence.0,
                                stream: automation_stream(chunk.stream),
                                bytes: chunk.bytes,
                            });
                            if full {
                                break;
                            }
                            self.record_failure(AutomationSourceFailureKind::FeedRejected);
                            anyhow::bail!("Automation external Runtime rejected its source; pending bytes and raw owner must be retained.");
                        }
                    }
                }
                AutomationViewEvent::Eof {
                    after_sequence,
                    stream,
                } => {
                    let descriptor = usize::from(stream == AutomationStream::Stderr);
                    if after_sequence != self.sequence.0 || self.eof[descriptor] {
                        self.pending = Some(AutomationViewEvent::Eof {
                            after_sequence,
                            stream,
                        });
                        self.record_failure(AutomationSourceFailureKind::EofOrder);
                        anyhow::bail!("Automation EOF order changed; source retained.");
                    }
                    self.eof[descriptor] = true;
                }
                AutomationViewEvent::Exit {
                    after_sequence,
                    code,
                } => {
                    if self.eof != [true; 2] || after_sequence != self.sequence.0 {
                        self.pending = Some(AutomationViewEvent::Exit {
                            after_sequence,
                            code,
                        });
                        self.record_failure(AutomationSourceFailureKind::ExitOrder);
                        anyhow::bail!("Automation exit preceded its final bytes or pipe EOFs.");
                    }
                    let finished = match self.output.as_mut() {
                        Some(output) => output.try_finish(self.sequence),
                        None => Err(ExternalFinishError::Closed),
                    };
                    match finished {
                        Ok(()) => {
                            self.finish_queued = true;
                            break;
                        }
                        Err(error) => {
                            let full = matches!(error, ExternalFinishError::Full);
                            self.pending = Some(AutomationViewEvent::Exit {
                                after_sequence,
                                code,
                            });
                            if full {
                                break;
                            }
                            self.record_failure(AutomationSourceFailureKind::FinishRejected);
                            anyhow::bail!("Automation view finish was not accepted; source and raw cleanup proof remain separate.");
                        }
                    }
                }
            }
        }
        Ok(())
    }
    /// Parser processed ordered Finish. The canonical parser remains available
    /// for Show after raw exit; this NEVER proves raw-process cleanup or Close.
    pub fn finished(&self, runtime: &Runtime) -> bool {
        self.failure.is_none()
            && self.finish_queued
            && runtime.identity() == self.identity
            && runtime.external_status().is_some_and(|status| {
                status.finished && !status.failed && status.source_sequence == self.sequence
            })
    }
    /// Only a joined original parser plus independently confirmed raw cleanup
    /// can turn this retained failure into a warning instead of resource failure.
    pub fn take_failed_evidence(
        &mut self,
        proof: AutomationFailedCleanupProof,
        runtime: &mut Runtime,
    ) -> Result<AutomationFailedSourceEvidence> {
        ensure!(
            !self.evidence_taken,
            "Automation failure evidence was already transferred."
        );
        ensure!(
            runtime.identity() == self.identity
                && proof.failure().identity() == Some(self.identity),
            "Automation failure cleanup identity changed."
        );
        ensure!(
            self.failure == Some(proof.failure()),
            "Automation failure cleanup evidence changed."
        );
        ensure!(
            runtime.poll_external_closed()?,
            "Original automation parser-thread join is not confirmed."
        );
        self.evidence_taken = true;
        Ok(AutomationFailedSourceEvidence::new(
            proof,
            self.pending.take(),
        ))
    }
    /// Sole-producer cancellation, never UI Close. Raw ownership/cleanup is
    /// unchanged. Dropping the real sender makes the Runtime honestly fail.
    pub fn abort_source(
        &mut self,
        reason: AutomationSourceAbortReason,
    ) -> Result<AutomationSourceFailure> {
        ensure!(
            !self.finish_queued,
            "An already finished source must be retired normally."
        );
        let kind = match reason {
            AutomationSourceAbortReason::StartupCancelled => {
                AutomationSourceFailureKind::StartupCancelled
            }
            AutomationSourceAbortReason::RawSpawnFailed => {
                AutomationSourceFailureKind::RawSpawnFailed
            }
        };
        self.record_failure(kind);
        self.failure
            .context("Automation source cancellation evidence is missing.")
    }
    #[cfg(test)]
    pub(crate) fn disconnect_for_test(&mut self) {
        self.output.take();
    }
    /// Requested abort intent only, not owner-processed failure or original join.
    pub fn abort_intent(&self) -> Option<ExternalAbortEvidence> {
        self.abort_intent
    }
    pub fn failure(&self) -> Option<AutomationSourceFailure> {
        self.failure
    }
    pub(super) fn record_failure(&mut self, kind: AutomationSourceFailureKind) {
        let (pending, bytes) = match &self.pending {
            Some(AutomationViewEvent::Data {
                sequence, bytes, ..
            }) => (Some(SourceSequence(*sequence)), bytes.len()),
            Some(
                AutomationViewEvent::Eof { after_sequence, .. }
                | AutomationViewEvent::Exit { after_sequence, .. },
            ) => (Some(SourceSequence(*after_sequence)), 0),
            None => (None, 0),
        };
        // A fatal producer failure disconnects the ORIGINAL source channel.
        // The Runtime then reports failed, never a fabricated ordered Finish.
        if let Some(output) = self.output.take() {
            let reason = match kind {
                AutomationSourceFailureKind::RawSpawnFailed => {
                    ExternalAbortReason::RawStartupFailed
                }
                AutomationSourceFailureKind::StartupCancelled => {
                    ExternalAbortReason::CancelledBeforeRawLaunch
                }
                _ => ExternalAbortReason::ProducerFailed,
            };
            self.abort_intent = Some(output.abort(reason));
        }
        if self.failure.is_none() {
            self.failure = Some(AutomationSourceFailure::new(
                Some(self.identity),
                self.sequence,
                pending,
                bytes,
                kind,
            ));
        }
    }
    pub fn identity(&self) -> Identity {
        self.identity
    }
}
fn external_stream(stream: AutomationStream) -> ExternalStream {
    match stream {
        AutomationStream::Stdout => ExternalStream::Stdout,
        AutomationStream::Stderr => ExternalStream::Stderr,
    }
}
fn automation_stream(stream: ExternalStream) -> AutomationStream {
    match stream {
        ExternalStream::Stdout => AutomationStream::Stdout,
        ExternalStream::Stderr => AutomationStream::Stderr,
    }
}
