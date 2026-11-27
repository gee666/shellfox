use std::collections::VecDeque;

use crate::{Chunk, Lease};

const CHUNK_BYTES: usize = 4096;
const CREDIT_BYTES: usize = 128 * 1024;
const PENDING_CHUNKS: usize = 256;
const REPLAY_BYTES: usize = 1024 * 1024;
/// As in 0.9: shells often write in small pieces, so the byte bound decides.
const REPLAY_CHUNKS: usize = 8192;
const RECOVERY_REQUIRED: &str = "Output replay expired; ANSI screen recovery is required.";

/// One controller's credit, backed only by a bounded raw-byte history.
/// No terminal snapshot format is produced or interpreted here.
#[derive(Default)]
pub(crate) struct Delivery {
    owner: Option<Lease>,
    epoch: u64,
    sequence: u64,
    history: VecDeque<Chunk>,
    history_bytes: usize,
    initialized: bool,
    recovery_pending: Option<u64>,
    replay_lost_reported: bool,
    sent: u64,
    acked: u64,
    pending: VecDeque<(u64, usize)>,
    pending_bytes: usize,
}

impl Delivery {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn claim(&mut self, controller: u64, takeover: bool) -> Result<Lease, String> {
        if self
            .owner
            .as_ref()
            .is_some_and(|owner| owner.controller != controller)
            && !takeover
        {
            return Err("Terminal control belongs to another controller.".into());
        }
        let epoch = self
            .epoch
            .checked_add(1)
            .ok_or("Control epoch exhausted.")?;
        self.epoch = epoch;
        self.owner = Some(Lease { controller, epoch });
        self.initialized = false;
        self.recovery_pending = None;
        self.replay_lost_reported = false;
        self.sent = 0;
        self.acked = 0;
        self.pending.clear();
        self.pending_bytes = 0;
        Ok(Lease { controller, epoch })
    }

    pub(crate) fn release(&mut self, lease: Lease) -> Result<(), String> {
        self.check(lease)?;
        self.owner = None;
        self.initialized = false;
        self.recovery_pending = None;
        self.pending.clear();
        self.pending_bytes = 0;
        self.sent = 0;
        self.acked = 0;
        self.replay_lost_reported = false;
        Ok(())
    }

    pub(crate) fn check(&self, lease: Lease) -> Result<(), String> {
        match &self.owner {
            Some(owner) if owner.controller == lease.controller && owner.epoch == lease.epoch => {
                Ok(())
            }
            _ => Err("Current terminal control lease required.".into()),
        }
    }

    /// Split without decoding UTF-8. Return the last sequence; empty input is a no-op.
    /// Check overflow before changing history so failure cannot append a partial input.
    #[cfg(test)]
    pub(crate) fn append(&mut self, bytes: Vec<u8>) -> Result<u64, String> {
        self.append_bytes(&bytes)
    }
    pub(crate) fn append_bytes(&mut self, bytes: &[u8]) -> Result<u64, String> {
        let count = u64::try_from(bytes.len().div_ceil(CHUNK_BYTES))
            .map_err(|_| "Output sequence exhausted.")?;
        self.sequence
            .checked_add(count)
            .ok_or("Output sequence exhausted.")?;
        for bytes in bytes.chunks(CHUNK_BYTES) {
            while self.history_bytes + bytes.len() > REPLAY_BYTES
                || self.history.len() >= REPLAY_CHUNKS
            {
                if let Some(oldest) = self.history.pop_front() {
                    self.history_bytes -= oldest.bytes.len();
                }
            }
            self.sequence += 1;
            self.history_bytes += bytes.len();
            self.history.push_back(Chunk {
                sequence: self.sequence,
                bytes: bytes.to_vec(),
            });
        }
        Ok(self.sequence)
    }

    pub(crate) fn acknowledge(&mut self, lease: Lease, sequence: u64) -> Result<(), String> {
        self.check(lease)?;
        if sequence < self.acked || sequence > self.sent {
            return Err("Acknowledgement is outside the delivered sequence range.".into());
        }
        self.acked = sequence;
        if self.recovery_pending == Some(sequence) {
            self.recovery_pending = None;
        }
        while self
            .pending
            .front()
            .is_some_and(|pending| pending.0 <= sequence)
        {
            if let Some((_, bytes)) = self.pending.pop_front() {
                self.pending_bytes -= bytes;
            }
        }
        Ok(())
    }

    /// Call only after producing the bounded ANSI screen at the current sequence.
    /// Recovery replaces credit, but its barrier must be ACKed even at sequence 0.
    pub(crate) fn recover(&mut self, lease: Lease) -> Result<u64, String> {
        self.check(lease)?;
        self.sent = self.sequence;
        self.pending.clear();
        self.pending_bytes = 0;
        self.initialized = true;
        self.recovery_pending = Some(self.sent);
        self.replay_lost_reported = false;
        Ok(self.sent)
    }

    /// The first replay in an epoch establishes the caller's resume cursor.
    /// Thereafter only `after == sent` is accepted: retries/replacement need a new
    /// claim, not a credit reset or an implicit ACK. Returns a credit-bounded prefix.
    pub(crate) fn replay(&mut self, lease: Lease, after: u64) -> Result<Vec<Chunk>, String> {
        self.check(lease)?;
        if after > self.sequence {
            return Err("Replay cursor is ahead of current output.".into());
        }
        if self
            .history
            .front()
            .is_some_and(|first| after < first.sequence - 1)
        {
            return Err(RECOVERY_REQUIRED.into());
        }
        if self.initialized && after != self.sent {
            return Err("Replay must continue after the last sent sequence; claim a fresh lease to replace it.".into());
        }
        if !self.initialized {
            self.sent = after;
            self.acked = after;
            self.initialized = true;
        }
        let mut chunks = Vec::new();
        while let Some(chunk) = self.next_live() {
            chunks.push(chunk);
        }
        Ok(chunks)
    }

    /// Require initial replay/recovery; stop on credit exhaustion or history gaps.
    /// Never jump to the retained tail. `replay(lease, sent)` reports a gap explicitly.
    pub(crate) fn next_live(&mut self) -> Option<Chunk> {
        self.owner.as_ref()?;
        if !self.initialized
            || self.recovery_pending.is_some()
            || self.pending.len() >= PENDING_CHUNKS
        {
            return None;
        }
        let next = self.sent.checked_add(1)?;
        let chunk = self.chunk(next)?;
        if self.pending_bytes + chunk.bytes.len() > CREDIT_BYTES {
            return None;
        }
        let chunk = Chunk {
            sequence: chunk.sequence,
            bytes: chunk.bytes.clone(),
        };
        self.pending_bytes += chunk.bytes.len();
        self.pending.push_back((chunk.sequence, chunk.bytes.len()));
        self.sent = chunk.sequence;
        Some(chunk)
    }

    /// Commit live credit only after the shared event channel accepts this chunk.
    pub(crate) fn pump_live(&mut self, emit: impl FnOnce(Chunk) -> bool) -> bool {
        if self.owner.is_none()
            || !self.initialized
            || self.recovery_pending.is_some()
            || self.pending.len() >= PENDING_CHUNKS
        {
            return false;
        }
        let Some(next) = self.sent.checked_add(1) else {
            return false;
        };
        let Some(chunk) = self.chunk(next) else {
            return false;
        };
        if self.pending_bytes + chunk.bytes.len() > CREDIT_BYTES {
            return false;
        }
        let sequence = chunk.sequence;
        let bytes = chunk.bytes.len();
        if !emit(chunk.clone()) {
            return false;
        }
        self.pending_bytes += bytes;
        self.pending.push_back((sequence, bytes));
        self.sent = sequence;
        true
    }

    /// Report a live history gap once per claim/recovery, independently of credit.
    /// Fresh claims need an explicit replay; recovery must first be ACKed.
    pub(crate) fn take_replay_lost(&mut self) -> bool {
        if !self.initialized || self.recovery_pending.is_some() || self.replay_lost_reported {
            return false;
        }
        let lost = self
            .history
            .front()
            .is_some_and(|first| self.sent < first.sequence - 1);
        self.replay_lost_reported = lost;
        lost
    }

    pub(crate) fn current_sequence(&self) -> u64 {
        self.sequence
    }

    /// Shared replay journal. Observers keep credit metadata, never another ring.
    pub(crate) fn first_sequence(&self) -> Option<u64> {
        self.history.front().map(|chunk| chunk.sequence)
    }

    pub(crate) fn chunk(&self, sequence: u64) -> Option<&Chunk> {
        let first = self.first_sequence()?;
        let index = usize::try_from(sequence.checked_sub(first)?).ok()?;
        self.history.get(index)
    }

    pub(crate) fn history_from(&self, skip: usize) -> impl Iterator<Item = &Chunk> {
        self.history.iter().skip(skip)
    }

    pub(crate) fn last_sent(&self) -> u64 {
        self.sent
    }

    pub(crate) fn lease(&self) -> Option<Lease> {
        self.owner.as_ref().map(|owner| Lease {
            controller: owner.controller,
            epoch: owner.epoch,
        })
    }
}

#[cfg(test)]
#[path = "delivery_tests.rs"]
mod tests;
