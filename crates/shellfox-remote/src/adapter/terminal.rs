use super::*;

#[derive(Clone, Debug)]
pub struct AttachmentFacts {
    pub generation: core::Generation,
    pub control_epoch: u64,
    pub delivery: AttachmentDelivery,
}
#[derive(Clone, Debug)]
pub enum AttachmentDelivery {
    Blocked,
    Owned(ReplayFacts),
}
#[derive(Clone, Copy, Debug)]
pub enum RecoveryCapability {
    Replay,
    /// Caller has verified the existing v1 ANSI restoration contract.
    AnsiV1,
    VisibleScreen,
    FormattedActiveBufferV1,
    GhosttyBinary,
}
#[derive(Clone, Debug)]
pub struct AnsiChunk {
    pub sequence: u64,
    /// Already stream-decoded UTF-8, never a lossy decode of an arbitrary PTY chunk.
    pub data: String,
}
#[derive(Clone, Debug)]
pub struct ReplayFacts {
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub chunks: Vec<AnsiChunk>,
    pub truncated: bool,
    pub recovery: RecoveryCapability,
    pub state: wire::ScreenState,
    pub exit_code: Option<i64>,
    pub cols: u16,
    pub rows: u16,
    pub lifetime: wire::TerminalLifetime,
}

pub fn terminal_attachment(
    snapshot: &core::Snapshot,
    tab_id: core::TabId,
    facts: Option<&AttachmentFacts>,
) -> AdapterResult<wire::TerminalAttachment> {
    let (session, _) = membership(snapshot, tab_id)?;
    let facts = required(facts, "terminal attachment")?;
    wire_identity(facts.generation)?;
    let attachment = match &facts.delivery {
        AttachmentDelivery::Blocked => wire::TerminalAttachment::Blocked {
            tab_id,
            session_id: session.id,
            generation: facts.generation,
            control_epoch: facts.control_epoch,
        },
        AttachmentDelivery::Owned(replay) => {
            safe_integer(replay.exit_code)?;
            let snapshot = match replay.recovery {
                RecoveryCapability::Replay => false,
                RecoveryCapability::AnsiV1 => true,
                RecoveryCapability::VisibleScreen
                | RecoveryCapability::FormattedActiveBufferV1
                | RecoveryCapability::GhosttyBinary => {
                    return Err(AdapterError::CapabilityUnavailable {
                        capability: "v1 ANSI terminal recovery",
                    });
                }
            };
            ensure(
                !matches!(replay.state, wire::ScreenState::Open) || replay.exit_code.is_none(),
                "open terminal has an exit code",
            )?;
            wire::TerminalAttachment::Owned(wire::OwnedAttachment {
                tab_id,
                session_id: session.id,
                generation: facts.generation,
                control_epoch: facts.control_epoch,
                first_sequence: replay.first_sequence,
                last_sequence: replay.last_sequence,
                chunks: replay
                    .chunks
                    .iter()
                    .map(|chunk| wire::TerminalData {
                        kind: wire::DataTag::Data,
                        tab_id,
                        generation: facts.generation,
                        sequence: chunk.sequence,
                        data: chunk.data.clone(),
                    })
                    .collect(),
                truncated: replay.truncated,
                snapshot: Some(snapshot),
                state: replay.state,
                exit_code: replay.exit_code,
                cols: replay.cols,
                rows: replay.rows,
                lifetime: replay.lifetime,
                settled_at: Some(session.settled_at.clone()),
            })
        }
    };
    checked(attachment, "terminal attachment")
}

#[derive(Clone, Debug)]
pub enum TerminalEventFacts {
    Settlement {
        generation: core::Generation,
    },
    Activity {
        busy: bool,
    },
    Data {
        generation: core::Generation,
        control_epoch: u64,
        sequence: u64,
        data: String,
    },
    Exit {
        generation: core::Generation,
        control_epoch: u64,
        exit_code: Option<i64>,
        signal: Option<i64>,
        last_sequence: u64,
    },
    Error {
        generation: core::Generation,
        control_epoch: u64,
        error: wire::AppError,
    },
    Control {
        generation: core::Generation,
        control_epoch: u64,
        control: wire::Control,
    },
}

pub fn terminal_event(
    snapshot: &core::Snapshot,
    tab_id: core::TabId,
    facts: Option<&TerminalEventFacts>,
) -> AdapterResult<wire::HostEvent> {
    let (session, _) = membership(snapshot, tab_id)?;
    let event = match required(facts, "terminal event")? {
        TerminalEventFacts::Settlement { generation } => wire::TerminalEvent::Settlement {
            tab_id,
            generation: *generation,
            settled_at: session.settled_at.clone(),
        },
        TerminalEventFacts::Activity { busy } => wire::TerminalEvent::Activity {
            tab_id,
            busy: *busy,
        },
        TerminalEventFacts::Data {
            generation,
            control_epoch,
            sequence,
            data,
        } => wire::TerminalEvent::Data {
            tab_id,
            generation: *generation,
            control_epoch: *control_epoch,
            sequence: *sequence,
            data: data.clone(),
        },
        TerminalEventFacts::Exit {
            generation,
            control_epoch,
            exit_code,
            signal,
            last_sequence,
        } => wire::TerminalEvent::Exit {
            tab_id,
            generation: *generation,
            control_epoch: *control_epoch,
            exit_code: *exit_code,
            signal: *signal,
            last_sequence: *last_sequence,
        },
        TerminalEventFacts::Error {
            generation,
            control_epoch,
            error,
        } => wire::TerminalEvent::Error {
            tab_id,
            generation: *generation,
            control_epoch: *control_epoch,
            error: error.clone(),
        },
        TerminalEventFacts::Control {
            generation,
            control_epoch,
            control,
        } => wire::TerminalEvent::Control {
            tab_id,
            generation: *generation,
            control_epoch: *control_epoch,
            control: *control,
        },
    };
    match &event {
        wire::TerminalEvent::Activity { .. } => {}
        wire::TerminalEvent::Exit {
            generation,
            exit_code,
            signal,
            ..
        } => {
            wire_identity(generation)?;
            safe_integer(*exit_code)?;
            safe_integer(*signal)?;
        }
        wire::TerminalEvent::Settlement { generation, .. }
        | wire::TerminalEvent::Data { generation, .. }
        | wire::TerminalEvent::Error { generation, .. }
        | wire::TerminalEvent::Control { generation, .. } => wire_identity(generation)?,
    }
    checked(wire::HostEvent::Terminal { event }, "terminal event")
}

fn membership(
    snapshot: &core::Snapshot,
    tab_id: core::TabId,
) -> AdapterResult<(&core::Session, &core::Tab)> {
    let mut found = None;
    for session in &snapshot.sessions {
        for tab in &session.tabs {
            if tab.id == tab_id {
                ensure(
                    found.is_none() && tab.session_id == session.id,
                    "terminal membership",
                )?;
                wire_identity(session.id)?;
                wire_identity(tab.id)?;
                optional_wire_timestamp(session.settled_at.as_deref())?;
                found = Some((session, tab));
            }
        }
    }
    required(found, "terminal metadata membership")
}
