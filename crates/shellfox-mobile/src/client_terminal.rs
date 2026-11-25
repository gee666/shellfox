use super::runtime::*;
use shellfox_client::{EventDisposition, ViewCursor};
use shellfox_protocol::*;
use std::collections::VecDeque;

pub(super) struct Binding {
    pub terminal: u64,
    pub cursor: Option<ViewCursor>,
    pub attaching: Option<(u64, AttachTerminal)>,
    pub pending_settlement: VecDeque<TerminalEvent>,
    /// Output that arrived while an attachment was in flight or not yet installed.
    pub early: VecDeque<TerminalEvent>,
    pub early_bytes: usize,
    pub connection: u64,
    pub tickets: VecDeque<(u64, u64)>,
    pub pending: VecDeque<(u64, Vec<u8>)>,
    pub consumed: Option<u64>,
    pub acked: Option<u64>,
    pub ack_inflight: Option<(u64, u64)>,
    pub reset_required: bool,
    pub reset_ticket: u64,
    pub required_grid: Option<(u16, u16)>,
    pub recovering: bool,
    pub recovery_ticket: Option<u64>,
    pub held: Option<TerminalAttachment>,
    pub lane: super::mutations::MutationLane,
    pub uploads: super::uploads::UploadBook,
}
#[path = "client_binding.rs"]
mod binding;
pub(super) use binding::{bind, release, unbind};
#[path = "client_settlement.rs"]
mod settlement;
use settlement::{apply_settlement, record_settlement};

pub(super) fn invalidate(s: &mut State) -> Result<()> {
    let end = s
        .next_reset_ticket
        .checked_add(s.bindings.len() as u64)
        .ok_or(BUSY)?;
    for binding in s.bindings.values_mut() {
        binding.reset_ticket = s.next_reset_ticket;
        s.next_reset_ticket += 1;
        binding.lane.fence();
        binding.uploads.fence();
        binding.cursor = None;
        binding.attaching = None;
        binding.pending_settlement.clear();
        binding.clear_early();
        binding.pending.clear();
        binding.tickets.clear();
        binding.consumed = None;
        binding.acked = None;
        binding.reset_required = true;
        binding.held = None;
        binding.required_grid = None;
        binding.recovering = false;
        binding.recovery_ticket = None;
        binding.ack_inflight = None;
    }
    s.next_reset_ticket = end;
    Ok(())
}
fn ensure_current(s: &State) -> Result<()> {
    let op = CURRENT.try_with(Clone::clone).map_err(|_| STALE)?;
    if s.closed || s.generation != op.generation || op.cancel.is_cancelled() {
        Err(STALE)
    } else {
        Ok(())
    }
}
fn install(b: &mut Binding, value: &TerminalAttachment, connection: u64) -> Result<()> {
    b.cursor = Some(ViewCursor::from_attachment(value, connection).map_err(|_| PROTOCOL)?);
    b.connection = connection;
    apply_settlement(b, connection)?;
    b.lane.open()?;
    b.uploads.fence();
    b.tickets.clear();
    b.pending.clear();
    b.consumed = None;
    b.acked = None;
    b.recovering = matches!(value, TerminalAttachment::Owned(_));
    b.recovery_ticket = None;
    b.ack_inflight = None;
    if let TerminalAttachment::Owned(a) = value {
        for chunk in &a.chunks {
            b.pending
                .push_back((chunk.sequence, chunk.data.as_bytes().to_vec()));
        }
        replay_early(b, connection)?;
        flush(b)?;
    }
    b.clear_early();
    Ok(())
}
impl Binding {
    pub(super) fn clear_early(&mut self) {
        self.early.clear();
        self.early_bytes = 0;
    }
}
/// Hold output for a tab whose attachment is still settling; install replays it.
fn hold_early(b: &mut Binding, event: &TerminalEvent) -> bool {
    if b.attaching.is_none() && b.held.is_none() {
        return false;
    }
    if let TerminalEvent::Data { data, .. } = event {
        // Past the bound, live output after install shows a gap and reattaches.
        if b.early.len() < crate::MAX_CHUNKS && b.early_bytes + data.len() <= crate::MAX_QUEUE {
            b.early_bytes += data.len();
            b.early.push_back(event.clone());
        }
        return true;
    }
    false
}
/// Deliver held output that follows the snapshot. Stop at a gap: the next live
/// event then reports it and the view reattaches as usual.
fn replay_early(b: &mut Binding, connection: u64) -> Result<()> {
    let cursor = b.cursor.as_mut().ok_or(STALE)?;
    for event in b.early.drain(..) {
        let mut trial = cursor.clone();
        let disposition = trial.observe(&event, connection).map_err(|_| STALE)?;
        if matches!(disposition, EventDisposition::Reattach) {
            break;
        }
        *cursor = trial;
        if let (EventDisposition::Deliver, TerminalEvent::Data { sequence, data, .. }) =
            (disposition, event)
        {
            b.pending.push_back((sequence, data.into_bytes()));
        }
    }
    Ok(())
}
#[path = "client_recovery.rs"]
mod recovery;
pub(super) use recovery::{ready, ready_handle, reset_done, reset_grid, reset_ticket};
fn flush(binding: &mut Binding) -> Result<()> {
    let shared = crate::engine::lookup(binding.terminal).map_err(|e| e as i32)?;
    let mut queue = shared.queue.lock().map_err(|_| PANIC)?;
    while let Some((host, bytes)) = binding.pending.front() {
        let ticket = queue.accepted.checked_add(1).ok_or(BUSY)?;
        match queue.submit(ticket, bytes) {
            Ok(()) => {
                binding.tickets.push_back((ticket, *host));
                binding.pending.pop_front();
            }
            Err(crate::Status::Busy) => break,
            Err(status) => return Err(status as i32),
        }
    }
    if binding.recovering && binding.pending.is_empty() && binding.recovery_ticket.is_none() {
        binding.recovery_ticket = Some(queue.accepted);
    }
    Ok(())
}
pub(super) fn attachment(mobile: &Mobile, value: &TerminalAttachment) -> Result<serde_json::Value> {
    let tab = match value {
        TerminalAttachment::Owned(a) => a.tab_id,
        TerminalAttachment::Blocked { tab_id, .. } => *tab_id,
    };
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    if s.closed {
        return Err(STALE);
    }
    ensure_current(&s)?;
    let ticket = s.next_reset_ticket;
    s.next_reset_ticket = ticket.checked_add(1).ok_or(BUSY)?;
    let operation = CURRENT.try_with(|op| op.id).map_err(|_| STALE)?;
    let b = s.bindings.get_mut(&tab).ok_or(INVALID)?;
    if b.attaching.as_ref().map(|(id, _)| *id) != Some(operation) {
        return Err(STALE);
    }
    if b.reset_required && b.held.is_some() {
        return Err(BUSY);
    }
    b.reset_ticket = ticket;
    b.lane.fence();
    b.uploads.fence();
    b.cursor = None;
    b.pending.clear();
    b.tickets.clear();
    b.required_grid = match value {
        TerminalAttachment::Owned(a) => Some((a.cols, a.rows)),
        _ => b.required_grid,
    };
    b.held = Some(value.clone());
    b.reset_required = true;
    b.recovering = false;
    b.recovery_ticket = None;
    // Keep the protocol's omission/null distinctions. Only high-volume replay
    // bytes are removed from this native/JS presentation result.
    let mut metadata = serde_json::to_value(value).map_err(|_| PROTOCOL)?;
    metadata.as_object_mut().ok_or(PROTOCOL)?.remove("chunks");
    Ok(metadata)
}
pub(super) fn poll_ready(mobile: &Mobile) -> Result<()> {
    let s = mobile.state.lock().map_err(|_| PANIC)?;
    for b in s.bindings.values() {
        if b.reset_required
            || !b.pending.is_empty()
            || b.tickets.len() >= crate::MAX_CHUNKS
            || (b.recovering && !ready(b)?)
        {
            return Err(BUSY);
        }
    }
    Ok(())
}
pub(super) fn events(mobile: &Mobile, response: PollResponse) -> Result<serde_json::Value> {
    let connection = mobile.client.state().connection_id;
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    if s.closed {
        return Err(STALE);
    }
    ensure_current(&s)?;
    let mut coarse = Vec::new();
    for event in response.events {
        match event {
            HostEvent::Changed { event } => {
                coarse.push(serde_json::json!({"type":"changed","event":event}))
            }
            // Activity is tab status for the session lists, not terminal output:
            // it has no generation and needs no attached view.
            HostEvent::Terminal {
                event: event @ TerminalEvent::Activity { .. },
            } => coarse.push(serde_json::json!({"type":"terminal","event":event})),
            HostEvent::Terminal { event } => {
                let tab = match &event {
                    TerminalEvent::Data { tab_id, .. }
                    | TerminalEvent::Exit { tab_id, .. }
                    | TerminalEvent::Error { tab_id, .. }
                    | TerminalEvent::Control { tab_id, .. }
                    | TerminalEvent::Activity { tab_id, .. }
                    | TerminalEvent::Settlement { tab_id, .. } => tab_id,
                };
                let Some(b) = s.bindings.get_mut(tab) else {
                    continue;
                };
                let recorded = record_settlement(b, &event)?;
                if hold_early(b, &event) {
                    continue;
                }
                let Some(cursor) = b.cursor.as_mut() else {
                    if recorded {
                        coarse.push(serde_json::json!({"type":"terminal","event":event}));
                    }
                    continue;
                };
                match cursor.observe(&event, connection).map_err(|_| STALE)? {
                    EventDisposition::Ignore => continue,
                    EventDisposition::Reattach => {
                        b.lane.fence();
                        b.uploads.fence();
                        coarse.push(serde_json::json!({"type":"reattach","request":cursor.recovery_request(connection).map_err(|_| STALE)?}));
                        continue;
                    }
                    EventDisposition::Blocked => {
                        b.lane.fence();
                        b.uploads.fence();
                        b.pending.clear();
                        b.tickets.clear();
                        b.consumed = None;
                    }
                    EventDisposition::Deliver => {}
                }
                if let TerminalEvent::Data { sequence, data, .. } = event {
                    b.pending.push_back((sequence, data.into_bytes()));
                    flush(b)?;
                } else {
                    coarse.push(serde_json::json!({"type":"terminal","event":event}));
                }
            }
        }
    }
    Ok(serde_json::json!({"ok":true,"value":{"events":coarse}}))
}
pub(super) fn consumed(mobile: &Mobile, terminal: u64) -> Result<AcknowledgeTerminal> {
    let connection = mobile.client.state().connection_id;
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    let b = s
        .bindings
        .values_mut()
        .find(|b| b.terminal == terminal)
        .ok_or(STALE)?;
    if b.reset_required {
        return Err(STALE);
    }
    let shared = crate::engine::lookup(terminal).map_err(|e| e as i32)?;
    let consumed = shared.queue.lock().map_err(|_| PANIC)?.consumed;
    while b
        .tickets
        .front()
        .is_some_and(|(ticket, _)| *ticket <= consumed)
    {
        b.consumed = b.tickets.pop_front().map(|(_, host)| host);
    }
    flush(b)?;
    let sequence = b.consumed.ok_or(EMPTY)?;
    if b.acked.is_some_and(|acked| acked >= sequence) {
        return Err(EMPTY);
    }
    if b.ack_inflight.is_some() {
        return Err(BUSY);
    }
    let ack = b
        .cursor
        .clone()
        .ok_or(STALE)?
        .consumed(sequence, connection)
        .map_err(|_| STALE)?;
    b.ack_inflight = Some((sequence, connection));
    Ok(ack)
}
pub(super) fn resized(
    mobile: &Mobile,
    terminal: u64,
    cols: u16,
    rows: u16,
) -> Result<ResizeTerminal> {
    let connection = mobile.client.state().connection_id;
    let s = mobile.state.lock().map_err(|_| PANIC)?;
    let b = s
        .bindings
        .values()
        .find(|b| b.terminal == terminal)
        .ok_or(STALE)?;
    if b.attaching.is_some() || b.reset_required {
        return Err(BUSY);
    }
    b.cursor
        .as_ref()
        .ok_or(STALE)?
        .resize(cols, rows, connection)
        .map_err(|_| STALE)
}
pub(super) fn written(
    mobile: &Mobile,
    terminal: u64,
    data: String,
    user_initiated: bool,
) -> Result<WriteTerminal> {
    let connection = mobile.client.state().connection_id;
    let s = mobile.state.lock().map_err(|_| PANIC)?;
    let b = s
        .bindings
        .values()
        .find(|b| b.terminal == terminal)
        .ok_or(STALE)?;
    if b.attaching.is_some() || b.reset_required {
        return Err(BUSY);
    }
    b.cursor
        .as_ref()
        .ok_or(STALE)?
        .write(data, Some(user_initiated), connection)
        .map_err(|_| STALE)
}
pub(super) fn ack_finished(
    mobile: &Mobile,
    ack: &AcknowledgeTerminal,
    connection: u64,
    success: bool,
) {
    if let Ok(mut s) = mobile.state.lock() {
        if let Some(b) = s.bindings.get_mut(&ack.tab_id) {
            if b.connection != connection || b.ack_inflight != Some((ack.sequence, connection)) {
                return;
            }
            b.ack_inflight = None;
            if success {
                if let Some(cursor) = b.cursor.as_mut() {
                    if let Ok(expected) = cursor.clone().consumed(ack.sequence, connection) {
                        if expected.generation == ack.generation
                            && expected.control_epoch == ack.control_epoch
                            && cursor.consumed(ack.sequence, connection).is_ok()
                        {
                            b.acked = Some(b.acked.unwrap_or(0).max(ack.sequence));
                        }
                    }
                }
            }
        }
    }
}
