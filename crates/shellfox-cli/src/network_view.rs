//! Network state in the 0.9 shapes (src/shared/remote-schemas.ts) for
//! `network`, `machines`, `approvals` and `remote api`. `host network` prints
//! the host's own record. Fields this version added are kept: extra fields
//! don't break 0.9 consumers, renamed values and fields do.
use anyhow::Result;
use serde_json::{json, Map, Value};
use shellfox_service::remote::RemoteOperatorState;
use std::time::{Duration, UNIX_EPOCH};

pub fn state(state: &RemoteOperatorState) -> Result<Value> {
    Ok(project(serde_json::to_value(state)?))
}

/// `machines list`: the peers this host has confirmed as Shellfox hosts.
pub fn machines(state: &RemoteOperatorState) -> Result<Value> {
    let mut view = self::state(state)?;
    let peers = view["peers"].as_array_mut().map(std::mem::take);
    let hosts = peers
        .unwrap_or_default()
        .into_iter()
        .filter(|peer| peer["hostConfirmed"] == true);
    Ok(Value::Array(hosts.collect()))
}

pub fn approvals(state: &RemoteOperatorState) -> Result<Value> {
    let mut view = self::state(state)?;
    Ok(json!({ "approvals": view["approvals"].take(), "pending": view["pending"].take() }))
}

fn project(mut state: Value) -> Value {
    kebab(&mut state["status"]);
    let error = state["error"]["message"].take();
    state["error"] = error;
    let approvals = rows(&mut state["approvals"]);
    let approved: Vec<Value> = approvals.iter().map(|a| a["peerId"].clone()).collect();
    state["approvals"] = approvals.into_iter().map(approval).collect();
    let mut peers = rows(&mut state["peers"]);
    for peer in &mut peers {
        kebab(&mut peer["connection"]);
        peer["approved"] = Value::Bool(approved.contains(&peer["id"]));
    }
    let pending = rows(&mut state["pending"]);
    state["pending"] = pending
        .into_iter()
        .map(|request| pending_peer(request, &peers))
        .collect();
    state["peers"] = Value::Array(peers);
    state
}

fn rows(value: &mut Value) -> Vec<Value> {
    match value.take() {
        Value::Array(rows) => rows,
        _ => Vec::new(),
    }
}

/// `loginRequired` → `login-required`, `awaitingApproval` → `awaiting-approval`.
fn kebab(value: &mut Value) {
    if let Some(text) = value.as_str() {
        let mut out = String::with_capacity(text.len() + 2);
        for c in text.chars() {
            if c.is_ascii_uppercase() {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        }
        *value = Value::String(out);
    }
}

/// `{peerId, hostname, approvedAt}` plus this version's `permission`.
fn approval(approval: Value) -> Value {
    let millis = approval["approvedAtMillis"].as_u64().unwrap_or_default();
    let approved_at = crate::time_text::iso(UNIX_EPOCH + Duration::from_millis(millis));
    let mut row = Map::new();
    for field in ["peerId", "hostname"] {
        row.insert(field.into(), approval[field].clone());
    }
    row.insert("approvedAt".into(), approved_at.into());
    if let Some(permission) = approval.get("permission") {
        row.insert("permission".into(), permission.clone());
    }
    Value::Object(row)
}

/// 0.9 listed a request as its network peer. The host reports only the ID,
/// name and client count, so the address and presence come from the peer list.
fn pending_peer(request: Value, peers: &[Value]) -> Value {
    let peer = peers.iter().find(|peer| peer["id"] == request["id"]);
    let field = |name: &str, fallback: Value| peer.map_or(fallback, |peer| peer[name].clone());
    json!({
        "id": request["id"],
        "hostname": request["hostname"],
        "addresses": field("addresses", json!([])),
        "online": field("online", json!(false)),
        "clients": request["clients"],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const PEER: &str = "nodeA";
    fn host_state() -> RemoteOperatorState {
        let peer = |id: &str, connection: &str, confirmed: bool| {
            json!({
                "id": id, "hostname": format!("{id}-host"), "addresses": ["100.64.0.1"],
                "online": true, "remembered": true, "hostConfirmed": confirmed,
                "connection": connection, "lastError": null, "attemptId": null
            })
        };
        serde_json::from_value(json!({
            "enabled": true, "hostname": "here", "status": "loginRequired",
            "loginUrl": null,
            "error": { "code": "NATIVE_UNAVAILABLE", "message": "Helper stopped.", "retryable": true },
            "peers": [peer(PEER, "awaitingApproval", true), peer("nodeB", "connected", false)],
            "pending": [
                { "id": PEER, "hostname": "nodeA-host", "clients": 1 },
                { "id": "nodeC", "hostname": "gone", "clients": 0 }
            ],
            "approvals": [{
                "peerId": PEER, "hostname": "nodeA-host", "approvedAtMillis": 951_782_400_123u64,
                "permission": "write", "fileRoots": []
            }],
            "attempts": [], "cleanupRetained": false,
            "incomingLegacyTerminalAttachment": false, "terminalCapabilityReason": "",
            "rootAccount": false
        }))
        .unwrap()
    }
    #[test]
    fn state_uses_0_9_values_and_keeps_new_fields() {
        let view = state(&host_state()).unwrap();
        assert_eq!(view["status"], "login-required");
        assert_eq!(view["error"], "Helper stopped.");
        assert_eq!(view["peers"][0]["connection"], "awaiting-approval");
        assert_eq!(view["peers"][0]["approved"], true);
        assert_eq!(view["peers"][1]["approved"], false);
        assert_eq!(
            view["approvals"],
            json!([{ "peerId": PEER, "hostname": "nodeA-host",
                     "approvedAt": "2000-02-29T00:00:00.123Z", "permission": "write" }])
        );
        assert_eq!(
            view["pending"],
            json!([
                { "id": PEER, "hostname": "nodeA-host", "addresses": ["100.64.0.1"], "online": true, "clients": 1 },
                { "id": "nodeC", "hostname": "gone", "addresses": [], "online": false, "clients": 0 }
            ])
        );
        assert_eq!(view["cleanupRetained"], false);
    }
    #[test]
    fn listings_project_the_same_records() {
        let mut host = host_state();
        host.error = None;
        assert_eq!(state(&host).unwrap()["error"], Value::Null);
        let machines = machines(&host).unwrap();
        assert_eq!(machines.as_array().map(Vec::len), Some(1));
        assert_eq!(machines[0]["connection"], "awaiting-approval");
        let approvals = approvals(&host).unwrap();
        assert_eq!(approvals["pending"][0]["online"], true);
        assert_eq!(
            approvals["approvals"][0]["approvedAt"],
            "2000-02-29T00:00:00.123Z"
        );
    }
}
