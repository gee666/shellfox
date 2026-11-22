use super::*;

pub(super) fn finish(mobile: &Mobile, op: &Operation, result: Result<serde_json::Value>) {
    if mobile.is_observer() {
        super::super::observer::finish_event(mobile, op, result);
        return;
    }
    if let Ok(mut s) = mobile.state.lock() {
        let Some(reserved) = s.operations.remove(&op.id) else {
            return;
        };
        s.reserved_bytes -= reserved.reservation;
        if s.closed {
            return;
        }
        let result = if op.cancel.is_cancelled() {
            Err(CANCELLED)
        } else if op.generation != s.generation {
            Err(STALE)
        } else {
            result
        };
        for binding in s.bindings.values_mut() {
            if binding
                .attaching
                .as_ref()
                .is_some_and(|(id, _)| *id == op.id)
            {
                binding.attaching = None;
                if binding.held.is_none() {
                    binding.pending_settlement.clear();
                }
            }
        }
        let Ok(client) = mobile.legacy() else {
            return;
        };
        let state = client.state();
        if state.status != ConnectionStatus::Connected {
            for binding in s.bindings.values_mut() {
                binding.lane.fence();
                binding.uploads.fence();
                binding.cursor = None;
                binding.attaching = None;
                binding.pending_settlement.clear();
                binding.pending.clear();
                binding.tickets.clear();
            }
        }
        let status = match state.status {
            ConnectionStatus::Disconnected => "disconnected",
            ConnectionStatus::Connecting => "connecting",
            ConnectionStatus::AwaitingApproval => "awaitingApproval",
            ConnectionStatus::Rejected => "rejected",
            ConnectionStatus::Connected => "connected",
            ConnectionStatus::Reconnecting => "reconnecting",
            ConnectionStatus::Suspended => "suspended",
        };
        let permission = match state.permission {
            Permission::Unknown => "unknown",
            Permission::AwaitingApproval => "awaitingApproval",
            Permission::Approved => "approved",
            Permission::Rejected => "rejected",
        };
        let (code, value) = match result {
            Ok(v) => (0, v),
            Err(e) => (e, serde_json::Value::Null),
        };
        let reset: Vec<_> = s
            .bindings
            .values()
            .filter(|b| b.reset_required)
            .map(|b| b.terminal.to_string())
            .collect();
        let reset_tickets: std::collections::BTreeMap<_, _> = s
            .bindings
            .values()
            .filter(|b| b.reset_required)
            .map(|b| (b.terminal.to_string(), b.reset_ticket.to_string()))
            .collect();
        let reset_geometry: std::collections::BTreeMap<_, _> = s
            .bindings
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
        let recovery_ready: Vec<_> = s
            .bindings
            .values()
            .filter(|b| b.recovering && super::super::terminal::ready(b).unwrap_or(false))
            .map(|b| b.terminal.to_string())
            .collect();
        let event = serde_json::json!({"resetGeometry":reset_geometry,"recoveryReady":recovery_ready,"operationId":op.id.to_string(),"internal":op.origin == CompletionOrigin::NativeInternal,"status":code,"result":value,"resetTerminals":reset,"resetTickets":reset_tickets,
            "connection":{"status":status,"permission":permission,"connectionId":state.connection_id.to_string(),
                "retryAfterMs":state.retry_after.map(|d| d.as_millis() as u64)}});
        if let Ok(bytes) = serde_json::to_vec(&event) {
            s.event_bytes += bytes.len();
            s.events.push_back(bytes);
            drop(s);
            mobile.wake();
        }
    }
}
