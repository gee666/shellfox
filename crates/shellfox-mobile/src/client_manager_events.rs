use crate::client::runtime::*;
use shellfox_client::manager_v2::Status;
pub(in crate::client) fn finish(
    mobile: &Mobile,
    operation: &Operation,
    result: Result<serde_json::Value>,
) {
    let Ok(client) = mobile.manager() else { return };
    let witness = client.binding_witness().ok();
    let binding = witness.as_ref().map(|w| w.binding().clone());
    let status = match client.status() {
        Status::Disconnected => "disconnected",
        Status::Discovered => "discovered",
        Status::Connecting => "connecting",
        Status::Connected => "connected",
        Status::LegacyAvailable => "legacyAvailable",
        Status::AwaitingApproval => "awaitingApproval",
        Status::Rejected => "rejected",
        Status::Reconnecting => "reconnecting",
    };
    let views = {
        let Ok(mut s) = mobile.state.lock() else {
            return;
        };
        let Some(reserved) = s.operations.remove(&operation.id) else {
            return;
        };
        s.reserved_bytes -= reserved.reservation;
        if s.closed {
            return;
        }
        s.managers.values().cloned().collect::<Vec<_>>()
    };
    // Never take a per-view slot while holding the Mobile registry mutex.
    for b in &views {
        for slot in [&b.attaching, &b.acking] {
            if let Ok(mut pending) = slot.lock() {
                if pending.as_ref().map(|op| op.id) == Some(operation.id) {
                    *pending = None;
                }
            }
        }
        if binding.is_none() {
            b.retire();
        }
    }
    let ready: Vec<_> = views
        .iter()
        .filter(|b| b.ready().unwrap_or(false))
        .map(|b| b.terminal.to_string())
        .collect();
    let retired: Vec<_> = views
        .iter()
        .filter(|b| b.state.lock().map_or(true, |state| state.retired))
        .map(|b| b.terminal.to_string())
        .collect();
    let Ok(mut s) = mobile.state.lock() else {
        return;
    };
    if s.closed {
        return;
    }
    let result = if operation.cancel.is_cancelled() {
        Err(CANCELLED)
    } else if s.generation != operation.generation {
        Err(STALE)
    } else {
        result
    };
    let (code, value) = match result {
        Ok(value) => (0, value),
        Err(error) => (error, serde_json::Value::Null),
    };
    let event = serde_json::json!({"profile":"manager-v2","operationId":operation.id.to_string(),"internal":operation.origin==CompletionOrigin::NativeInternal,"status":code,"result":value,"connection":{"status":status,"binding":binding},"resetTerminals":[],"resetTickets":{},"resetGeometry":{},"recoveryReady":ready,"retiredTerminals":retired});
    if let Ok(bytes) = serde_json::to_vec(&event) {
        s.event_bytes += bytes.len();
        s.events.push_back(bytes);
    }
}
