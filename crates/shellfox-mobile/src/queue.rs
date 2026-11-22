use crate::{
    engine::{Result, Status},
    MAX_CHUNK, MAX_CHUNKS, MAX_QUEUE,
};
use std::collections::VecDeque;

pub(crate) enum Payload {
    Bytes(Vec<u8>),
}
impl Payload {
    pub fn len(&self) -> usize {
        match self {
            Self::Bytes(bytes) => bytes.len(),
        }
    }
}
#[derive(Default)]
pub(crate) struct Queue {
    pub chunks: VecDeque<(u64, Payload)>,
    pub bytes: usize,
    pub accepted: u64,
    pub consumed: u64,
    pub closed: bool,
    pub failed: bool,
}
impl Queue {
    pub fn submit(&mut self, sequence: u64, bytes: &[u8]) -> Result<()> {
        if self.closed {
            return Err(Status::Stale);
        }
        if self.failed {
            return Err(Status::Engine);
        }
        if bytes.is_empty()
            || bytes.len() > MAX_CHUNK
            || self.accepted.checked_add(1) != Some(sequence)
        {
            return Err(Status::Invalid);
        }
        if self.chunks.len() >= MAX_CHUNKS || self.bytes + bytes.len() > MAX_QUEUE {
            return Err(Status::Busy);
        }
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(bytes.len())
            .map_err(|_| Status::Busy)?;
        owned.extend_from_slice(bytes);
        self.chunks.try_reserve(1).map_err(|_| Status::Busy)?;
        self.bytes += bytes.len();
        self.chunks.push_back((sequence, Payload::Bytes(owned)));
        self.accepted = sequence;
        Ok(())
    }
    /// Discard unconsumed LOCAL work on an explicit fresh-recovery attempt.
    /// This never advances consumption or acknowledges any host sequence.
    pub fn fail(&mut self) {
        self.failed = true;
        self.chunks.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_order_and_rejects_gaps_duplicates_empty() {
        let mut q = Queue::default();
        assert_eq!(q.submit(2, b"x"), Err(Status::Invalid));
        q.submit(1, b"a").unwrap();
        assert_eq!(q.submit(1, b"a"), Err(Status::Invalid));
        assert_eq!(q.submit(2, b""), Err(Status::Invalid));
        q.submit(2, b"b").unwrap();
        assert!(matches!(q.chunks.pop_front(),Some((1,Payload::Bytes(bytes)))if bytes==b"a"));
        assert!(matches!(q.chunks.pop_front(),Some((2,Payload::Bytes(bytes)))if bytes==b"b"));
    }
    #[test]
    fn backpressure_never_advances_cursor() {
        let mut q = Queue::default();
        let chunk = vec![b'x'; MAX_CHUNK];
        for n in 1..=16 {
            q.submit(n, &chunk).unwrap();
        }
        assert_eq!(q.submit(17, b"x"), Err(Status::Busy));
        assert_eq!(q.accepted, 16);
        assert_eq!(q.bytes, MAX_QUEUE);
        q.fail();
        assert_eq!(q.submit(17, b"x"), Err(Status::Engine));
        assert_eq!(q.consumed, 0);
    }
    #[test]
    fn bounds_chunk_count_and_closed_handles() {
        let mut q = Queue::default();
        for n in 1..=MAX_CHUNKS {
            q.submit(n as u64, b"x").unwrap();
        }
        assert_eq!(q.submit(65, b"x"), Err(Status::Busy));
        q.closed = true;
        assert_eq!(q.submit(65, b"x"), Err(Status::Stale));
    }
}
