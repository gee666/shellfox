//! One-shot output reads for CLI automation: `api attachTerminal` and the
//! read-only `api readTerminal` / `terminals read`. Output positions are the
//! runtime journal's sequences, so they stay stable across CLI processes.
use super::*;
use shellfox_protocol::{
    AppError, DataTag, ErrorCode, ReadTerminal, ScreenState, SnapshotChunk, TerminalData,
    TerminalLifetime, TerminalRead,
};
use shellfox_runtime::{OutputRead, ReadOutput, RuntimeError};

/// 0.9's attachment bound. More output after the cursor is replaced by a snapshot.
const READ_BYTES: usize = 256 * 1024;
const SNAPSHOT_CHUNK_BYTES: usize = 16 * 1024;

/// A 0.9 `attachTerminal` from one CLI process.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ManagerAttachment {
    /// This process controls the terminal until it exits, as in 0.9.
    Owned {
        control_epoch: u64,
        read: Box<TerminalRead>,
    },
    Blocked {
        session_id: shellfox_core::SessionId,
        generation: Generation,
        control_epoch: u64,
    },
}

fn app(code: ErrorCode, message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(AppError {
        code,
        message: message.into(),
        retryable: false,
    })
}

/// Splits UTF-8 across chunk boundaries: the incomplete tail of one chunk
/// starts the next, so a cursor between them never garbles a character.
fn split_tail(bytes: &[u8]) -> usize {
    let start = bytes.len().saturating_sub(3);
    let Some(lead) = (start..bytes.len())
        .rev()
        .find(|&at| bytes[at] & 0xc0 != 0x80)
    else {
        return bytes.len();
    };
    let incomplete =
        std::str::from_utf8(&bytes[lead..]).is_err_and(|error| error.error_len().is_none());
    if incomplete {
        lead
    } else {
        bytes.len()
    }
}

fn replay(
    tab: TabId,
    generation: Generation,
    after: u64,
    chunks: Vec<shellfox_runtime::Chunk>,
) -> Vec<TerminalData> {
    let mut carry = Vec::new();
    let mut out = Vec::new();
    for chunk in chunks {
        let cursor = chunk.sequence <= after;
        carry.extend_from_slice(&chunk.bytes);
        let end = split_tail(&carry);
        let rest = carry.split_off(end);
        let data = String::from_utf8_lossy(&carry).into_owned();
        carry = rest;
        if cursor || data.is_empty() {
            continue;
        }
        out.push(TerminalData {
            kind: DataTag::Data,
            tab_id: tab,
            generation,
            sequence: chunk.sequence,
            data,
        });
    }
    out
}

fn snapshot_chunks(bytes: &[u8]) -> Vec<SnapshotChunk> {
    let text = String::from_utf8_lossy(bytes);
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let end = text.floor_char_boundary((start + SNAPSHOT_CHUNK_BYTES).min(text.len()));
        chunks.push(SnapshotChunk {
            index: chunks.len() as u64 + 1,
            data: text[start..end].to_owned(),
        });
        start = end;
    }
    chunks
}

impl Service {
    /// Reads output without taking or changing control. Never starts a shell.
    pub(crate) fn read_terminal(&mut self, input: &ReadTerminal) -> Result<TerminalRead> {
        let tab = input.tab_id;
        let snapshot = self.host().snapshot()?;
        let session = snapshot
            .sessions
            .iter()
            .find(|s| s.tabs.iter().any(|t| t.id == tab))
            .ok_or_else(|| app(ErrorCode::NotFound, "Terminal not found."))?;
        let terminal = self.terminals.get(&tab).ok_or_else(|| {
            app(
                ErrorCode::NotFound,
                "No runtime terminal exists for this tab. Reads never start a shell.",
            )
        })?;
        let generation = terminal.runtime.identity().generation;
        let replaced = input.generation.is_some_and(|g| g != generation);
        let after = input.after_sequence.filter(|_| !replaced);
        let read = self.call(
            tab,
            Command::ReadOutput {
                after: after.unwrap_or(0),
                max_bytes: READ_BYTES,
            },
        );
        let OutputRead {
            last_sequence,
            cols,
            rows,
            output,
        } = match read {
            Ok(Response::OutputRead(read)) => read,
            Ok(_) => bail!("Unexpected runtime read response."),
            Err(error) => {
                return Err(match error.downcast_ref::<RuntimeError>() {
                    Some(e) if e.code == shellfox_runtime::ErrorCode::Validation => {
                        app(ErrorCode::Validation, e.message.clone())
                    }
                    _ => error,
                })
            }
        };
        let terminal = &self.terminals[&tab];
        let state = if terminal.runtime.phase() == shellfox_runtime::Phase::Closed {
            ScreenState::Closed
        } else {
            ScreenState::Open
        };
        let exit_code = self.completed_automation_exit_code(terminal);
        let mut read = TerminalRead {
            tab_id: tab,
            session_id: session.id,
            generation,
            first_sequence: last_sequence + 1,
            last_sequence,
            chunks: Vec::new(),
            truncated: false,
            snapshot: None,
            snapshot_sequence: None,
            snapshot_chunks: Vec::new(),
            replay_lost: None,
            state,
            exit_code,
            cols,
            rows,
            lifetime: TerminalLifetime::AppOwned,
            settled_at: Some(session.settled_at.clone()),
        };
        match output {
            ReadOutput::Replay(chunks) => {
                let from = after.unwrap_or(0);
                read.first_sequence = from + 1;
                read.chunks = replay(tab, generation, from, chunks);
                read.replay_lost = replaced.then_some(true);
            }
            ReadOutput::Screen(bytes) => {
                read.truncated = true;
                read.snapshot = Some(true);
                read.snapshot_sequence = Some(last_sequence);
                read.snapshot_chunks = snapshot_chunks(&bytes);
                read.replay_lost = (replaced || after.is_some()).then_some(true);
            }
        }
        Ok(read)
    }

    /// 0.9 `attachTerminal` for a local CLI call: it takes control only when no
    /// other view holds it, so it never starts a shell or takes the desktop's.
    pub(crate) fn manager_attachment(
        &mut self,
        input: &ReadTerminal,
        controller: u64,
    ) -> Result<ManagerAttachment> {
        let tab = input.tab_id;
        let terminal = self
            .terminals
            .get(&tab)
            .context("Terminal is not running.")?;
        let identity = terminal.runtime.identity();
        let control_epoch = terminal.control_epoch.max(1);
        let offer = match self.attach_manager(tab, controller, false) {
            Ok(offer) => offer,
            Err(error)
                if matches!(error.downcast_ref::<crate::grant::requests::GrantFailure>(),
                Some(crate::grant::requests::GrantFailure::Rejected(error)) if error.code == shellfox_runtime::ErrorCode::ControlRequired) =>
            {
                return Ok(ManagerAttachment::Blocked {
                    session_id: identity.session_id,
                    generation: identity.generation,
                    control_epoch,
                });
            }
            Err(error) => return Err(error),
        };
        let attachment = self.adopt_controller_grant(offer)?;
        match self.read_terminal(input) {
            Ok(read) => Ok(ManagerAttachment::Owned {
                control_epoch: attachment.epoch,
                read: Box::new(read),
            }),
            Err(error) => {
                let _ = self.detach_attachment(tab, &attachment);
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shellfox_runtime::Chunk;

    fn chunk(sequence: u64, bytes: &[u8]) -> Chunk {
        Chunk {
            sequence,
            bytes: bytes.to_vec(),
        }
    }
    fn texts(after: u64, chunks: Vec<Chunk>) -> Vec<(u64, String)> {
        replay(TabId::new(), Generation::new(), after, chunks)
            .into_iter()
            .map(|c| (c.sequence, c.data))
            .collect()
    }

    #[test]
    fn a_character_split_between_chunks_survives_any_cursor() {
        let heart = "💙".as_bytes();
        let chunks = || {
            vec![
                chunk(1, &[b"a", &heart[..2]].concat()),
                chunk(2, &[&heart[2..], b"b"].concat()),
            ]
        };
        assert_eq!(texts(0, chunks()), [(1, "a".into()), (2, "💙b".into())]);
        // Resuming after chunk 1 still completes the character from its tail.
        assert_eq!(texts(1, chunks()), [(2, "💙b".into())]);
    }

    #[test]
    fn invalid_bytes_decode_lossily_and_empty_chunks_are_skipped() {
        assert_eq!(
            texts(
                0,
                vec![
                    chunk(1, b"\xffok"),
                    chunk(2, &"é".as_bytes()[..1]),
                    chunk(3, &"é".as_bytes()[1..])
                ]
            ),
            [(1, "\u{fffd}ok".into()), (3, "é".into())]
        );
    }

    #[test]
    fn snapshot_pieces_are_indexed_not_sequenced() {
        let chunks = snapshot_chunks(&vec![b'x'; SNAPSHOT_CHUNK_BYTES + 1]);
        assert_eq!(chunks.iter().map(|c| c.index).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(chunks[1].data, "x");
    }
}
