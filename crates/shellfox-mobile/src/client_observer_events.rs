use super::{observer_delivery as delivery, runtime::*};
use shellfox_client::observer_v2::Status;

pub(super) fn finish_event(
    mobile: &Mobile,
    operation: &Operation,
    result: Result<serde_json::Value>,
) {
    let Ok(client) = mobile.observer() else {
        return;
    };
    let mut s = match mobile.state.lock() {
        Ok(s) => s,
        Err(_) => return,
    };
    let Some(reserved) = s.operations.remove(&operation.id) else {
        return;
    };
    s.reserved_bytes -= reserved.reservation;
    if s.closed {
        return;
    }
    let result = if operation.cancel.is_cancelled() {
        Err(CANCELLED)
    } else if operation.generation != s.generation {
        Err(STALE)
    } else {
        result
    };
    for b in s.observers.values_mut() {
        if b.attaching == Some(operation.id) {
            b.attaching = None;
        }
        if b.ack_operation.as_ref().map(|op| op.id) == Some(operation.id) {
            b.ack_operation = None;
        }
    }
    if s.observer_disconnect == Some(operation.id) {
        s.observer_disconnect = None;
    }
    let binding = client.binding();
    if binding.is_none()
        && s.observers.values().any(|b| b.cursor.is_some())
        && delivery::fence(&mut s).is_err()
    {
        return;
    }
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
    let reset: Vec<_> = s
        .observers
        .values()
        .filter(|b| b.reset_required)
        .map(|b| b.terminal.to_string())
        .collect();
    let tickets: std::collections::BTreeMap<_, _> = s
        .observers
        .values()
        .filter(|b| b.reset_required)
        .map(|b| (b.terminal.to_string(), b.reset_ticket.to_string()))
        .collect();
    let grids: std::collections::BTreeMap<_, _> = s
        .observers
        .values()
        .filter(|b| b.reset_required)
        .filter_map(|b| {
            b.required_grid.map(|(cols, rows)| {
                (
                    b.terminal.to_string(),
                    serde_json::json!({"cols":cols,"rows":rows}),
                )
            })
        })
        .collect();
    let ready: Vec<_> = s
        .observers
        .values()
        .filter(|b| {
            !b.reset_required
                && (b.recovery_acked
                    || b.ack.as_ref().is_some_and(|ack| {
                        matches!(
                            ack.ack,
                            shellfox_protocol::observer_v2::Ack::Recovery { .. }
                        )
                    }))
        })
        .map(|b| b.terminal.to_string())
        .collect();
    let (code, value) = match result {
        Ok(value) => (0, value),
        Err(error) => (error, serde_json::Value::Null),
    };
    let event = serde_json::json!({"profile":"observer-v2","operationId":operation.id.to_string(),
        "internal":operation.origin == CompletionOrigin::NativeInternal,"status":code,"result":value,
        "connection":{"status":status,"binding":binding},"resetTerminals":reset,"resetTickets":tickets,
        "resetGeometry":grids,"recoveryReady":ready});
    if let Ok(bytes) = serde_json::to_vec(&event) {
        s.event_bytes += bytes.len();
        s.events.push_back(bytes);
    }
}
