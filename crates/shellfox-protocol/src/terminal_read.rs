//! Output reads for one-shot automation (`api attachTerminal`, `api readTerminal`).
//! Local-only: the remote v1 wire keeps 0.9's strict attachment shape.
use crate::{validation::*, *};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadTerminal {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub tab_id: TabId,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::validation::optional")]
    pub after_sequence: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default, deserialize_with = "crate::ids::optional_wire_id")]
    pub generation: Option<Generation>,
}
impl Validate for ReadTerminal {
    fn validate(&self) -> Result<(), ValidationError> {
        self.after_sequence.map_or(Ok(()), sequence)
    }
}

/// One piece of a serialized screen. `index` counts pieces; it is not an output position.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotChunk {
    pub index: u64,
    pub data: String,
}

/// `firstSequence`, `lastSequence` and `chunks[].sequence` are output positions,
/// stable for one generation. Resume with `afterSequence: lastSequence`.
/// A snapshot is the screen at `snapshotSequence`, which equals `lastSequence`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalRead {
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub tab_id: TabId,
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub session_id: SessionId,
    #[serde(deserialize_with = "crate::ids::wire_id")]
    pub generation: Generation,
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub chunks: Vec<TerminalData>,
    /// Output before the reply was not replayed: a snapshot stands in for it.
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub snapshot_chunks: Vec<SnapshotChunk>,
    /// The requested cursor or generation could not be continued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_lost: Option<bool>,
    pub state: ScreenState,
    #[serde(deserialize_with = "crate::validation::nullable")]
    pub exit_code: Option<i64>,
    pub cols: u16,
    pub rows: u16,
    pub lifetime: TerminalLifetime,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::validation::present_nullable"
    )]
    pub settled_at: Option<Option<String>>,
}
impl Validate for TerminalRead {
    fn validate(&self) -> Result<(), ValidationError> {
        epoch(self.first_sequence)?;
        sequence(self.last_sequence)?;
        ensure(self.first_sequence <= self.last_sequence + 1 && self.chunks.len() <= 8192)?;
        ensure(matches!(self.lifetime, TerminalLifetime::AppOwned))?;
        if let Some(code) = self.exit_code {
            integer(code)?;
        }
        if let Some(value) = &self.settled_at {
            optional_time(value)?;
        }
        ensure(
            self.snapshot_sequence
                .is_none_or(|s| s == self.last_sequence),
        )?;
        ensure(self.snapshot.is_some() == self.snapshot_sequence.is_some())?;
        let mut previous = 0;
        let mut bytes = 0;
        for chunk in &self.chunks {
            chunk.validate()?;
            ensure(
                chunk.tab_id == self.tab_id
                    && chunk.generation == self.generation
                    && chunk.sequence >= self.first_sequence
                    && chunk.sequence <= self.last_sequence
                    && chunk.sequence > previous,
            )?;
            previous = chunk.sequence;
            bytes += chunk.data.len();
        }
        for (n, chunk) in self.snapshot_chunks.iter().enumerate() {
            ensure(chunk.index == n as u64 + 1 && !chunk.data.is_empty())?;
            ensure(chunk.data.len() <= 16384)?;
            bytes += chunk.data.len();
        }
        ensure(bytes <= 1024 * 1024)
    }
}
