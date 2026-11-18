use super::{observer_delivery as delivery, runtime::*};
use shellfox_protocol::observer_v2 as v2;
use std::sync::Arc;

pub(super) fn fence(s: &mut State) -> Result<()> {
    if s.closed {
        return Err(STALE);
    }
    s.generation = s.generation.checked_add(1).ok_or(BUSY)?;
    for operation in s.operations.values() {
        operation.cancel.cancel();
    }
    let pending: Vec<_> = s.requests.drain().collect();
    s.outbound.clear();
    for (id, request) in pending {
        if request.dispatched {
            s.cancels.push_back((request.generation, id));
        }
    }
    delivery::fence(s)
}
pub(super) fn disconnect(mobile: Arc<Mobile>) -> Result<u64> {
    mobile.observer()?;
    let executor = runtime()?;
    let operation = {
        let _dispatch = mobile.dispatch_gate.lock().map_err(|_| PANIC)?;
        let mut s = mobile.state.lock().map_err(|_| PANIC)?;
        if s.observer_disconnect.is_some() {
            return Err(BUSY);
        }
        fence(&mut s)?;
        s.observer_withdrawing = true;
        let operation = reserve_locked(&mut s, CompletionOrigin::Explicit, RESULT_RESERVE)?;
        s.observer_disconnect = Some(operation.id);
        operation
    };
    let id = operation.id;
    start(executor, mobile.clone(), operation, async move {
        let result = mobile
            .observer()?
            .disconnect()
            .await
            .map_err(super::observer::observer_error)?;
        serde_json::to_value(result).map_err(|_| PROTOCOL)
    });
    Ok(id)
}
pub(super) fn consumed(mobile: Arc<Mobile>, terminal: u64) -> Result<()> {
    let current = mobile.observer()?.binding().ok_or(STALE)?;
    let shared = crate::engine::lookup(terminal).map_err(|e| e as i32)?;
    if shared.owner != std::thread::current().id() {
        return Err(crate::Status::WrongThread as i32);
    }
    let executor = runtime()?;
    let (operation, ack) = {
        let mut s = mobile.state.lock().map_err(|_| PANIC)?;
        if s.closed || s.observer_withdrawing {
            return Err(STALE);
        }
        let b = s
            .observers
            .values_mut()
            .find(|b| b.terminal == terminal)
            .ok_or(STALE)?;
        if b.reset_required || b.cursor.is_none() {
            return Err(STALE);
        }
        if let Some(expected) = b.required_grid {
            if super::observer_owner::grid(terminal)? != expected {
                return Err(INVALID);
            }
        }
        let q = shared.queue.lock().map_err(|_| PANIC)?;
        if q.closed || q.failed {
            return Err(crate::Status::Engine as i32);
        }
        let consumed = q.consumed;
        drop(q);
        while b
            .tickets
            .front()
            .is_some_and(|(ticket, _)| *ticket <= consumed)
        {
            if let Some((_, Some(sequence))) = b.tickets.pop_front() {
                b.consumed = Some(sequence);
            }
        }
        delivery::flush(b)?;
        if b.ack_operation.is_some() {
            return Err(BUSY);
        }
        if b.ack.is_none() {
            if !delivery::parsed(b)? {
                return Err(EMPTY);
            }
            let cursor = b.cursor.as_mut().ok_or(STALE)?;
            let candidate = if !b.recovery_acked {
                cursor.consumed_recovery(cursor.recovery().recovery_id, &current)
            } else {
                let sequence = b.consumed.ok_or(EMPTY)?;
                if b.acked.is_some_and(|previous| sequence <= previous) {
                    return Err(EMPTY);
                }
                cursor.consumed_data(sequence, &current)
            }
            .map_err(|_| STALE)?;
            b.ack = Some(candidate);
        }
        let ack = b.ack.clone().ok_or(EMPTY)?;
        let operation = reserve_locked(&mut s, CompletionOrigin::NativeInternal, 64 * 1024)?;
        let b = s
            .observers
            .values_mut()
            .find(|b| b.terminal == terminal)
            .ok_or(STALE)?;
        b.ack_operation = Some(operation.clone());
        (operation, ack)
    };
    start(executor, mobile.clone(), operation, async move {
        let _slot = request_slot(&mobile).await?;
        let current = mobile.observer()?.binding().ok_or(STALE)?;
        {
            let s = mobile.state.lock().map_err(|_| PANIC)?;
            let cursor = s
                .observers
                .get(&ack.delivery.tab_id)
                .and_then(|b| b.cursor.as_ref())
                .ok_or(STALE)?;
            cursor.validate_ack(&ack, &current).map_err(|_| STALE)?;
        }
        let result = mobile
            .observer()?
            .execute(&ack)
            .await
            .map_err(super::observer::observer_error)?;
        if matches!(result, v2::Reply::Success(_)) {
            ack_success(&mobile, &current, &ack)?;
        }
        serde_json::to_value(result).map_err(|_| PROTOCOL)
    });
    Ok(())
}
fn ack_success(
    mobile: &Mobile,
    current: &v2::Binding,
    ack: &v2::AcknowledgeTerminal,
) -> Result<()> {
    if mobile.observer()?.binding().as_ref() != Some(current) {
        return Err(STALE);
    }
    let op = CURRENT.try_with(Clone::clone).map_err(|_| STALE)?;
    let mut s = mobile.state.lock().map_err(|_| PANIC)?;
    if s.closed || s.generation != op.generation || op.cancel.is_cancelled() {
        return Err(STALE);
    }
    let b = s.observers.get_mut(&ack.delivery.tab_id).ok_or(STALE)?;
    if b.ack_operation.as_ref().map(|op| op.id) != Some(op.id) {
        return Err(STALE);
    }
    b.cursor
        .as_ref()
        .ok_or(STALE)?
        .validate_ack(ack, current)
        .map_err(|_| STALE)?;
    if matches!(ack.ack, v2::Ack::Recovery { .. }) {
        b.recovery_acked = true;
    }
    b.acked = Some(ack.ack.sequence());
    b.ack = None;
    Ok(())
}
