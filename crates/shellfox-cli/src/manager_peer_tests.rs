//! `--host` over the real v1 client against a scripted peer on the v1 wire:
//! what crosses, what never leaves this machine, and how replies map to codes.
use super::*;
use crate::{
    manager_args::{ManagerCommand, PathOp, SessionOp, Target},
    manager_run::{execute, write_peer, Host, Options},
};
use serde_json::json;
use std::sync::{Arc, Mutex};

const TAB: &str = "22345678-1234-4234-8234-123456789abc";
const SESSION: &str = "12345678-1234-4234-8234-123456789abc";
const GENERATION: &str = "32345678-1234-4234-8234-123456789abc";

#[derive(Clone, Default)]
struct FakePeer {
    approved: bool,
    /// Every request that reached the peer: endpoint and method.
    seen: Arc<Mutex<Vec<String>>>,
}
impl OutboundTransport for FakePeer {
    fn send<'a>(
        &'a self,
        _: &'a AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        _: RequestLimits,
    ) -> TransportFuture<'a> {
        Box::pin(async move {
            let body: Value =
                serde_json::from_slice(&body).map_err(|_| TransportError::InvalidRequest)?;
            let method = body["method"].as_str().unwrap_or_default().to_owned();
            self.seen
                .lock()
                .unwrap()
                .push(format!("{endpoint:?} {method}"));
            let ok = |value: Value| json!({ "ok": true, "value": value });
            let reply = match (endpoint, method.as_str()) {
                (Endpoint::Pair, _) if self.approved => {
                    ok(json!({ "paired": true, "terminalControl": "exclusive-v1" }))
                }
                (Endpoint::Pair, _) => json!({ "ok": false, "error": {
                    "code": "APPROVAL_REQUIRED", "message": "Approve this device on the host.", "retryable": false } }),
                (Endpoint::Disconnect, _) => ok(json!({ "disconnected": true })),
                (_, "getHomeDirectory") => ok(json!({ "cwd": "/home/peer" })),
                (_, "resolveDirectory") => ok(
                    json!({ "cwd": format!("/home/peer/{}", body["payload"]["path"].as_str().unwrap_or_default()) }),
                ),
                (_, "attachTerminal") => ok(
                    json!({ "control": "blocked", "tabId": TAB, "sessionId": SESSION, "generation": GENERATION, "controlEpoch": 2 }),
                ),
                (_, "takeTerminalControl") => ok(json!({ "taken": true })),
                (_, "detachTerminal") => ok(json!({ "detached": true })),
                _ => {
                    json!({ "ok": false, "error": { "code": "NOT_FOUND", "message": "Session not found.", "retryable": false } })
                }
            };
            Ok(serde_json::to_vec(&reply).unwrap())
        })
    }
}

fn peer_id() -> AuthenticatedPeerId {
    AuthenticatedPeerId::new("n-build-box".to_owned()).unwrap()
}
fn options() -> Options {
    Options {
        json: true,
        yes: false,
        host: Some("build-box".into()),
    }
}
fn code(error: anyhow::Error) -> String {
    error
        .downcast_ref::<CliError>()
        .map(|e| e.code.to_string())
        .unwrap_or_default()
}

#[tokio::test]
async fn an_unapproved_peer_fails_with_its_own_code() {
    let peer = FakePeer::default();
    let error = Peer::pair(peer_id(), Box::new(peer.clone()))
        .await
        .err()
        .unwrap();
    assert_eq!(code(error), "APPROVAL_REQUIRED");
    assert_eq!(*peer.seen.lock().unwrap(), ["Pair "]);
}

#[tokio::test]
async fn context_and_local_only_methods_never_cross() {
    let peer = FakePeer {
        approved: true,
        ..Default::default()
    };
    let host = Host::Peer(Peer::pair(peer_id(), Box::new(peer.clone())).await.unwrap());
    let current = ManagerCommand::Session(SessionOp::Pin(true), Target::Current);
    let error = execute(&host, current, &options()).await.unwrap_err();
    assert_eq!(
        error.to_string(),
        "A remote session ID is required. Local terminal context is never sent to another host."
    );
    let focus = ManagerCommand::Session(SessionOp::Focus, Target::Id(SESSION.parse().unwrap()));
    assert_eq!(
        code(execute(&host, focus, &options()).await.unwrap_err()),
        "UNSUPPORTED"
    );
    let keep_awake = ManagerCommand::Api {
        method: "setKeepAwake".into(),
        payload: json!({ "active": true }),
    };
    assert_eq!(
        code(execute(&host, keep_awake, &options()).await.unwrap_err()),
        "UNSUPPORTED"
    );
    // Destructive requests still need --yes before anything is sent.
    let delete = ManagerCommand::Api {
        method: "deleteSession".into(),
        payload: json!({ "sessionId": SESSION }),
    };
    assert_eq!(
        code(execute(&host, delete, &options()).await.unwrap_err()),
        "VALIDATION"
    );
    assert_eq!(*peer.seen.lock().unwrap(), ["Pair "]);
}

#[tokio::test]
async fn requests_and_failures_round_trip_on_the_v1_wire() {
    let peer = FakePeer {
        approved: true,
        ..Default::default()
    };
    let host = Host::Peer(Peer::pair(peer_id(), Box::new(peer.clone())).await.unwrap());
    assert_eq!(
        host.call(crate::manager::parse("getHomeDirectory", json!({})).unwrap())
            .await
            .unwrap(),
        json!({ "cwd": "/home/peer" })
    );
    execute(
        &host,
        ManagerCommand::Path(PathOp::Resolve("src".into())),
        &options(),
    )
    .await
    .unwrap();
    let inspect = crate::manager::parse(
        "renameSession",
        json!({ "sessionId": SESSION, "title": "x" }),
    )
    .unwrap();
    assert_eq!(code(host.call(inspect).await.unwrap_err()), "NOT_FOUND");
    // A blocked terminal is never taken without --take, and is always detached.
    let identity = json!({ "tabId": TAB, "generation": GENERATION });
    let blocked = write_peer(&host, identity.clone(), "ls\r".into(), false)
        .await
        .unwrap_err();
    assert_eq!(code(blocked), "TERMINAL_CONTROL_REQUIRED");
    let taken = write_peer(&host, identity, "ls\r".into(), true)
        .await
        .unwrap_err();
    assert_eq!(code(taken), "TERMINAL_CONTROL_REQUIRED");
    let Host::Peer(peer_client) = host else {
        unreachable!()
    };
    peer_client.close().await;
    assert_eq!(
        *peer.seen.lock().unwrap(),
        [
            "Pair ",
            "Request getHomeDirectory",
            "Request resolveDirectory",
            "Request renameSession",
            "Request attachTerminal",
            "Request detachTerminal",
            "Request attachTerminal",
            "Request takeTerminalControl",
            "Request attachTerminal",
            "Request detachTerminal",
            "Disconnect ",
        ]
    );
}
