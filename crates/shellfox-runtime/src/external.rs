//! One bounded raw-byte source for a passive terminal view, never a process owner.
use crate::{GridLimits, RuntimeError};
use std::sync::{
    mpsc::{self, Receiver, SyncSender, TrySendError},
    Arc, Mutex,
};
pub const EXTERNAL_CHUNK_BYTES: usize = 32 * 1024;
pub const EXTERNAL_FEED_CAPACITY: usize = 8;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    OwnedPty,
    ExternalBytes,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceSequence(pub u64);
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeliverySequence(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalStream {
    Stdout,
    Stderr,
}
/// Raw bytes deliberately do not implement Debug or serialization.
pub struct ExternalChunk {
    pub sequence: SourceSequence,
    pub stream: ExternalStream,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone, Copy)]
pub struct ExternalConfig {
    pub cols: u16,
    pub rows: u16,
    pub limits: GridLimits,
}
impl Default for ExternalConfig {
    fn default() -> Self {
        Self {
            cols: 80,
            rows: 24,
            limits: GridLimits::default(),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryRange {
    pub first: DeliverySequence,
    pub last: DeliverySequence,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalAbortReason {
    RawStartupFailed,
    ProducerFailed,
    CancelledBeforeRawLaunch,
}
/// A producer disconnect request, not proof of EOF, raw exit or cleanup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalAbortEvidence {
    pub identity: crate::Identity,
    pub last_queued: SourceSequence,
    pub reason: ExternalAbortReason,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExternalStatus {
    /// Last accepted upstream raw chunk, not the segmented delivery sequence.
    pub source_sequence: SourceSequence,
    pub stream: Option<ExternalStream>,
    /// Delivery range produced by that source chunk. Zero before any input.
    pub delivery: Option<DeliveryRange>,
    pub last_delivery_sequence: DeliverySequence,
    /// Final stdout-then-stderr decoder flush, with no new raw source sequence.
    pub finish_delivery: Option<DeliveryRange>,
    /// Explicit ordered Finish was processed. This is NOT raw-process exit proof.
    pub finished: bool,
    pub closed: bool,
    pub failed: bool,
    pub abort: Option<ExternalAbortEvidence>,
}
pub enum ExternalFeedError {
    Invalid {
        chunk: ExternalChunk,
        error: RuntimeError,
    },
    Full(ExternalChunk),
    Closed(ExternalChunk),
}
impl std::fmt::Debug for ExternalFeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (label, chunk) = match self {
            Self::Invalid { chunk, .. } => ("Invalid", chunk),
            Self::Full(chunk) => ("Full", chunk),
            Self::Closed(chunk) => ("Closed", chunk),
        };
        f.debug_struct(label)
            .field("sequence", &chunk.sequence)
            .field("stream", &chunk.stream)
            .field("bytes", &chunk.bytes.len())
            .finish()
    }
}
#[derive(Debug)]
pub enum ExternalFinishError {
    Invalid(RuntimeError),
    Full,
    Closed,
}
pub(crate) enum ExternalMessage {
    Data(ExternalChunk),
    Finish(SourceSequence),
}
/// A single non-clone producer capability, taken from Runtime exactly once.
/// Full returns the exact chunk. Retain it and stop reading the raw pipe until
/// retry succeeds; never drop, renumber or feed a truncated tail as a new source.
/// Chunk length and retained Vec allocation capacity are both bounded to 32768.
pub struct ExternalSource {
    launch: Arc<crate::launch::LaunchState>,
    launch_binding: Option<shellfox_terminal::NativeLaunchBinding>,
    identity: crate::Identity,
    status: Arc<Mutex<ExternalStatus>>,
    sender: SyncSender<ExternalMessage>,
    signal: WakeOnDrop,
    queued: SourceSequence,
    finished: bool,
}
impl ExternalSource {
    /// One native factory correlation binding. It grants neither launch nor share permission.
    pub fn take_launch_binding(&mut self) -> Option<shellfox_terminal::NativeLaunchBinding> {
        self.launch_binding.take()
    }
    pub fn bind_launch_evidence(
        &mut self,
        evidence: shellfox_terminal::NativeLaunchEvidence,
    ) -> Result<(), RuntimeError> {
        self.launch.bind(evidence)
    }
    /// Sole producer authority only. Actual sender drop is observed in FIFO
    /// order by the parser; no Finish/EOF or process-cleanup proof is invented.
    pub fn abort(self, reason: ExternalAbortReason) -> ExternalAbortEvidence {
        let evidence = ExternalAbortEvidence {
            identity: self.identity,
            last_queued: self.queued,
            reason,
        };
        self.status
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .abort = Some(evidence);
        drop(self);
        evidence
    }
    pub fn try_feed(&mut self, chunk: ExternalChunk) -> Result<(), ExternalFeedError> {
        if self.finished {
            return Err(ExternalFeedError::Closed(chunk));
        }
        if chunk.bytes.is_empty()
            || chunk.bytes.len() > EXTERNAL_CHUNK_BYTES
            || chunk.bytes.capacity() > EXTERNAL_CHUNK_BYTES
            || self.queued.0.checked_add(1) != Some(chunk.sequence.0)
        {
            return Err(ExternalFeedError::Invalid { chunk, error: RuntimeError::validation("External chunks must be nonempty, <=32768 bytes and contiguous from source sequence 1.") });
        }
        let sequence = chunk.sequence;
        match self.sender.try_send(ExternalMessage::Data(chunk)) {
            Ok(()) => {
                self.queued = sequence;
                self.signal.0.notify();
                Ok(())
            }
            Err(TrySendError::Full(ExternalMessage::Data(chunk))) => {
                Err(ExternalFeedError::Full(chunk))
            }
            Err(TrySendError::Disconnected(ExternalMessage::Data(chunk))) => {
                Err(ExternalFeedError::Closed(chunk))
            }
            Err(_) => unreachable!("the submitted message was Data"),
        }
    }
    /// Call only after both upstream pipe EOFs and every prior chunk was queued.
    /// Finishing this view never proves raw-process exit or owned cleanup.
    pub fn try_finish(&mut self, last: SourceSequence) -> Result<(), ExternalFinishError> {
        if self.finished {
            return Err(ExternalFinishError::Closed);
        }
        if last != self.queued {
            return Err(ExternalFinishError::Invalid(RuntimeError::validation(
                "External finish must follow the exact queued source sequence.",
            )));
        }
        match self.sender.try_send(ExternalMessage::Finish(last)) {
            Ok(()) => {
                self.finished = true;
                self.signal.0.notify();
                Ok(())
            }
            Err(TrySendError::Full(_)) => Err(ExternalFinishError::Full),
            Err(TrySendError::Disconnected(_)) => Err(ExternalFinishError::Closed),
        }
    }
}
/// Wakes the owner when the source is dropped. Declared after `sender`, so the
/// owner observes the disconnected channel when it wakes.
struct WakeOnDrop(Arc<crate::owner_wake::OwnerSignal>);
impl Drop for WakeOnDrop {
    fn drop(&mut self) {
        self.0.notify();
    }
}
pub(crate) struct ExternalOwner {
    pub launch: Arc<crate::launch::LaunchState>,
    pub display: crate::external_display::Display,
    pub receiver: Option<Receiver<ExternalMessage>>,
    pub status: Arc<Mutex<ExternalStatus>>,
}
#[cfg(test)]
pub(crate) fn channel() -> (ExternalSource, ExternalOwner) {
    channel_with_policy(
        crate::Identity {
            session_id: shellfox_core::SessionId::new(),
            tab_id: shellfox_core::TabId::new(),
            generation: shellfox_core::Generation::new(),
        },
        crate::ExternalDisplayPolicy::RawBytes,
        Arc::default(),
    )
}
pub(crate) fn channel_with_policy(
    identity: crate::Identity,
    policy: crate::ExternalDisplayPolicy,
    signal: Arc<crate::owner_wake::OwnerSignal>,
) -> (ExternalSource, ExternalOwner) {
    let (sender, receiver) = mpsc::sync_channel(EXTERNAL_FEED_CAPACITY);
    let status = Arc::new(Mutex::new(ExternalStatus::default()));
    #[cfg(target_os = "linux")]
    let purpose = crate::LaunchFactoryPurpose::RawProgram;
    #[cfg(not(target_os = "linux"))]
    let purpose = crate::LaunchFactoryPurpose::StandardRawCreateProcess;
    let (launch, launch_binding) = crate::launch::new(purpose);
    (
        ExternalSource {
            launch: Arc::clone(&launch),
            launch_binding: Some(launch_binding),
            identity,
            status: Arc::clone(&status),
            sender,
            signal: WakeOnDrop(signal),
            queued: SourceSequence(0),
            finished: false,
        },
        ExternalOwner {
            launch,
            display: crate::external_display::Display::new(policy),
            receiver: Some(receiver),
            status,
        },
    )
}

#[cfg(test)]
#[path = "external_tests.rs"]
mod tests;
