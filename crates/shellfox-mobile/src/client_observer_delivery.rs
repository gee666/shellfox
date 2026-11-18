use super::{runtime::*, terminal};
use shellfox_client::observer_v2::DeliveryCursor;
use shellfox_protocol::observer_v2 as v2;
use std::{collections::VecDeque, sync::Arc};

pub(super) struct Segment {
    pub bytes: Vec<u8>,
    pub sequence: Option<u64>,
}
pub(super) struct DeliveryBinding {
    pub terminal: u64,
    pub cursor: Option<DeliveryCursor>,
    pub attaching: Option<u64>,
    pub reset_required: bool,
    pub reset_ticket: u64,
    pub required_grid: Option<(u16, u16)>,
    pub pending: VecDeque<Segment>,
    pub tickets: VecDeque<(u64, Option<u64>)>,
    pub recovery_ticket: Option<u64>,
    pub recovery_acked: bool,
    pub consumed: Option<u64>,
    pub acked: Option<u64>,
    pub ack: Option<v2::AcknowledgeTerminal>,
    pub ack_operation: Option<Operation>,
}
impl DeliveryBinding {
    fn new(terminal: u64) -> Self {
        Self {
            terminal,
            cursor: None,
            attaching: None,
            reset_required: false,
            reset_ticket: 0,
            required_grid: None,
            pending: VecDeque::new(),
            tickets: VecDeque::new(),
            recovery_ticket: None,
            recovery_acked: false,
            consumed: None,
            acked: None,
            ack: None,
            ack_operation: None,
        }
    }
    pub fn clear(&mut self) {
        if let Some(op) = self.ack_operation.take() {
            op.cancel.cancel();
        }
        self.cursor = None;
        self.attaching = None;
        self.pending.clear();
        self.tickets.clear();
        self.required_grid = None;
        self.recovery_ticket = None;
        self.recovery_acked = false;
        self.consumed = None;
        self.acked = None;
        self.ack = None;
    }
}
impl Drop for DeliveryBinding {
    fn drop(&mut self) {
        self.clear();
    }
}
pub(super) fn bind(mobile: &Mobile, tab: v2::Id, engine: u64) -> Result<()> {
    mobile.observer()?;
    let shared = crate::engine::lookup(engine).map_err(|e| e as i32)?;
    let q = shared.queue.lock().map_err(|_| PANIC)?;
    if q.closed || q.failed {
        return Err(STALE);
    }
    if q.accepted != q.consumed {
        return Err(BUSY);
    }
    drop(q);
    let mut owners = terminal::owners().lock().map_err(|_| PANIC)?;
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    if s.closed {
        return Err(STALE);
    }
    if owners.contains_key(&engine) || s.observers.contains_key(&tab) || s.observers.len() >= 32 {
        return Err(BUSY);
    }
    owners.insert(engine, mobile.id);
    s.observers.insert(tab, DeliveryBinding::new(engine));
    Ok(())
}
pub(super) fn unbind(mobile: &Mobile, engine: u64) -> Result<()> {
    mobile.observer()?;
    let mut owners = terminal::owners().lock().map_err(|_| PANIC)?;
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    let tab = s
        .observers
        .iter()
        .find(|(_, b)| b.terminal == engine)
        .map(|(tab, _)| *tab)
        .ok_or(STALE)?;
    if let Some(id) = s.observers.get(&tab).and_then(|b| b.attaching) {
        if let Some(op) = s.operations.get(&id) {
            op.cancel.cancel();
        }
    }
    s.observers.remove(&tab);
    owners.remove(&engine);
    Ok(())
}
pub(super) fn fence(s: &mut State) -> Result<()> {
    let end = s
        .next_reset_ticket
        .checked_add(s.observers.len() as u64)
        .ok_or(BUSY)?;
    for b in s.observers.values_mut() {
        b.clear();
        b.reset_ticket = s.next_reset_ticket;
        s.next_reset_ticket += 1;
        b.reset_required = true;
    }
    s.next_reset_ticket = end;
    Ok(())
}
pub(super) fn flush(b: &mut DeliveryBinding) -> Result<()> {
    let shared = crate::engine::lookup(b.terminal).map_err(|e| e as i32)?;
    let mut q = shared.queue.lock().map_err(|_| PANIC)?;
    while let Some(segment) = b.pending.front() {
        let ticket = q.accepted.checked_add(1).ok_or(BUSY)?;
        match q.submit(ticket, &segment.bytes) {
            Ok(()) => {
                b.tickets.push_back((ticket, segment.sequence));
                b.pending.pop_front();
            }
            Err(crate::Status::Busy) => break,
            Err(e) => return Err(e as i32),
        }
    }
    if b.recovery_ticket.is_none() && b.cursor.is_some() && b.pending.is_empty() {
        b.recovery_ticket = Some(q.accepted);
    }
    Ok(())
}
pub(super) fn parsed(b: &DeliveryBinding) -> Result<bool> {
    if b.reset_required || b.cursor.is_none() {
        return Ok(false);
    }
    let Some(ticket) = b.recovery_ticket else {
        return Ok(false);
    };
    let shared = crate::engine::lookup(b.terminal).map_err(|e| e as i32)?;
    let q = shared.queue.lock().map_err(|_| PANIC)?;
    Ok(!q.failed && !q.closed && q.consumed >= ticket)
}
pub(super) fn poll_ready(mobile: &Mobile) -> Result<()> {
    mobile.observer()?;
    let s = mobile.state.lock().map_err(|_| PANIC)?;
    if s.closed || s.observer_withdrawing {
        return Err(STALE);
    }
    for b in s.observers.values() {
        if b.reset_required
            || b.attaching.is_some()
            || !b.pending.is_empty()
            || b.tickets.len() >= crate::MAX_CHUNKS
            || (b.cursor.is_some() && !b.recovery_acked)
        {
            return Err(BUSY);
        }
    }
    Ok(())
}
pub(super) fn attach(mobile: Arc<Mobile>, request: v2::AttachTerminal) -> Result<u64> {
    let client = mobile.observer()?;
    if client.binding().is_none() {
        return Err(STALE);
    }
    let executor = runtime()?;
    let operation = {
        let mut s = mobile.state.lock().map_err(|_| PANIC)?;
        if s.observer_withdrawing {
            return Err(STALE);
        }
        let b = s.observers.get(&request.tab_id).ok_or(INVALID)?;
        if b.reset_required || b.attaching.is_some() || b.ack_operation.is_some() {
            return Err(BUSY);
        }
        let ticket = s.next_reset_ticket;
        let next = ticket.checked_add(1).ok_or(BUSY)?;
        let operation = reserve_locked(&mut s, CompletionOrigin::Explicit, RESULT_RESERVE)?;
        s.next_reset_ticket = next;
        let b = s.observers.get_mut(&request.tab_id).ok_or(STALE)?;
        b.clear();
        b.reset_required = true;
        b.reset_ticket = ticket;
        b.attaching = Some(operation.id);
        operation
    };
    let id = operation.id;
    start(executor, mobile.clone(), operation, async move {
        let _slot = request_slot(&mobile).await?;
        let result = mobile
            .observer()?
            .execute(&request)
            .await
            .map_err(super::observer::observer_error)?;
        match result {
            v2::Reply::Success(value) => {
                super::observer_flow::attachment(&mobile, request.tab_id, value)
            }
            v2::Reply::Failure(error) => {
                serde_json::to_value(v2::Reply::<v2::Attachment>::Failure(error))
                    .map_err(|_| PROTOCOL)
            }
        }
    });
    Ok(id)
}
