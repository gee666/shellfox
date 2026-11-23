use super::*;
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

fn owners() -> &'static Mutex<HashMap<u64, u64>> {
    static OWNERS: OnceLock<Mutex<HashMap<u64, u64>>> = OnceLock::new();
    OWNERS.get_or_init(|| Mutex::new(HashMap::new()))
}
pub(crate) fn bind(mobile: &Mobile, tab: TabId, terminal: u64) -> Result<()> {
    let shared = crate::engine::lookup(terminal).map_err(|e| e as i32)?;
    let q = shared.queue.lock().map_err(|_| PANIC)?;
    if q.closed || q.failed {
        return Err(STALE);
    }
    if q.accepted != q.consumed {
        return Err(BUSY);
    }
    drop(q);
    let mut owners = owners().lock().map_err(|_| PANIC)?;
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    if s.closed {
        return Err(STALE);
    }
    if s.bindings.len() >= 32 || owners.contains_key(&terminal) || s.bindings.contains_key(&tab) {
        return Err(BUSY);
    }
    s.bindings.insert(
        tab,
        Binding {
            terminal,
            cursor: None,
            attaching: None,
            pending_settlement: VecDeque::new(),
            early: VecDeque::new(),
            early_bytes: 0,
            connection: 0,
            tickets: VecDeque::new(),
            pending: VecDeque::new(),
            consumed: None,
            acked: None,
            reset_required: false,
            reset_ticket: 0,
            required_grid: None,
            recovering: false,
            recovery_ticket: None,
            held: None,
            ack_inflight: None,
            lane: super::super::mutations::MutationLane::default(),
            uploads: super::super::uploads::UploadBook::default(),
        },
    );
    owners.insert(terminal, mobile.id);
    Ok(())
}
pub(crate) fn unbind(mobile: &Mobile, terminal: u64) -> Result<()> {
    let mut owners = owners().lock().map_err(|_| PANIC)?;
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    let tab = s
        .bindings
        .iter()
        .find(|(_, b)| b.terminal == terminal)
        .map(|(t, _)| *t)
        .ok_or(STALE)?;
    if let Some((id, _)) = s.bindings.get(&tab).and_then(|b| b.attaching.as_ref()) {
        if let Some(operation) = s.operations.get(id) {
            operation.cancel.cancel();
        }
    }
    s.bindings.remove(&tab);
    owners.remove(&terminal);
    Ok(())
}
pub(crate) fn release(mobile: &Mobile) {
    if let Ok(mut owners) = owners().lock() {
        owners.retain(|_, client| *client != mobile.id);
    }
}
