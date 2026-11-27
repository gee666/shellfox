//! Read-only output reads for one-shot automation. They hold no lease and
//! leave every controller's and observer's credit untouched.
use crate::{delivery::Delivery, owner::Owner, *};

#[derive(Debug)]
pub struct OutputRead {
    /// The newest output sequence; the read covers everything up to it.
    pub last_sequence: u64,
    pub cols: u16,
    pub rows: u16,
    pub output: ReadOutput,
}
#[derive(Debug)]
pub enum ReadOutput {
    /// Retained output after the cursor, oldest first. The chunk at the cursor
    /// itself leads when retained, so a reader can finish a UTF-8 sequence it split.
    Replay(Vec<Chunk>),
    /// The cursor fell out of the replay window: the RemoteV1 screen at `last_sequence`.
    Screen(Vec<u8>),
}

impl Delivery {
    /// Chunks from `after` on, or None when output after it was evicted or
    /// exceeds `max_bytes`.
    pub(crate) fn retained_from(&self, after: u64, max_bytes: usize) -> Option<Vec<Chunk>> {
        let Some(first) = self.first_sequence() else {
            return (after == self.current_sequence()).then(Vec::new);
        };
        if after.saturating_add(1) < first {
            return None;
        }
        let skip = usize::try_from(after.max(first) - first).ok()?;
        let mut bytes = 0;
        let mut chunks = Vec::new();
        for chunk in self.history_from(skip) {
            if chunk.sequence > after {
                bytes += chunk.bytes.len();
                if bytes > max_bytes {
                    return None;
                }
            }
            chunks.push(chunk.clone());
        }
        Some(chunks)
    }
}

impl Owner {
    pub(crate) fn read_output(
        &mut self,
        after: u64,
        max_bytes: usize,
    ) -> Result<Response, RuntimeError> {
        let last_sequence = self.delivery.current_sequence();
        if after > last_sequence {
            return Err(RuntimeError::validation(
                "The requested sequence is ahead of this terminal.",
            ));
        }
        let output = match self.delivery.retained_from(after, max_bytes) {
            Some(chunks) => ReadOutput::Replay(chunks),
            None => ReadOutput::Screen(self.recovery_bytes(AnsiRecoveryCapability::RemoteV1)?),
        };
        let geometry = self
            .terminal
            .recovery_info()
            .map_err(crate::commands::engine)?;
        Ok(Response::OutputRead(OutputRead {
            last_sequence,
            cols: geometry.cols,
            rows: geometry.rows,
            output,
        }))
    }
}

#[cfg(test)]
mod tests {
    use crate::delivery::Delivery;

    fn journal(chunks: usize) -> Delivery {
        let mut delivery = Delivery::new();
        for n in 0..chunks {
            delivery.append(vec![b'a' + n as u8; 10]).unwrap();
        }
        delivery
    }
    fn sequences(chunks: Option<Vec<crate::Chunk>>) -> Option<Vec<u64>> {
        chunks.map(|chunks| chunks.iter().map(|c| c.sequence).collect())
    }

    #[test]
    fn reads_after_a_cursor_with_the_cursor_chunk_leading() {
        let delivery = journal(4);
        assert_eq!(
            sequences(delivery.retained_from(0, 100)),
            Some(vec![1, 2, 3, 4])
        );
        assert_eq!(
            sequences(delivery.retained_from(2, 100)),
            Some(vec![2, 3, 4])
        );
        assert_eq!(sequences(delivery.retained_from(4, 100)), Some(vec![4]));
        assert_eq!(
            sequences(Delivery::new().retained_from(0, 100)),
            Some(vec![])
        );
    }

    #[test]
    fn output_beyond_the_byte_bound_needs_a_screen() {
        let delivery = journal(4);
        assert_eq!(
            sequences(delivery.retained_from(2, 20)),
            Some(vec![2, 3, 4])
        );
        assert_eq!(delivery.retained_from(1, 20), None);
    }

    #[test]
    fn an_evicted_cursor_needs_a_screen() {
        let mut delivery = Delivery::new();
        // 1 MiB of 4 KiB chunks evicts the first ones.
        for _ in 0..300 {
            delivery.append(vec![b'x'; 4096]).unwrap();
        }
        let first = delivery.first_sequence().unwrap();
        assert!(first > 2);
        assert_eq!(delivery.retained_from(first - 2, usize::MAX), None);
        assert_eq!(
            sequences(delivery.retained_from(first - 1, usize::MAX)).map(|s| s[0]),
            Some(first)
        );
    }
}
