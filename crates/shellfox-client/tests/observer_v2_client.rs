use futures::{executor::block_on, task::noop_waker_ref};
use serde_json::{json, Value};
use shellfox_client::observer_v2::{
    ClientError, DiscoveryOutcome, FallbackPolicy, ObserverClient, Status,
};
use shellfox_protocol::{
    observer_v2::{self as wire, Id},
    *,
};
use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
#[path = "support/observer_v2_lifecycle.rs"]
mod lifecycle;
#[path = "support/observer_v2_negotiation.rs"]
mod negotiation;
fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../shellfox-protocol/tests/fixtures/observer_v2/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}
fn value(name: &str) -> Value {
    serde_json::from_slice(&fixture(name)).unwrap()
}
fn id(n: u8) -> Id {
    format!("{n}0000000-0000-4000-8000-000000000001")
        .parse()
        .unwrap()
}
enum Response {
    Bytes(Vec<u8>),
    Error(TransportError),
    Wait,
}
struct Call {
    endpoint: Endpoint,
    peer: String,
    body: Value,
    limits: RequestLimits,
}
#[derive(Clone, Default)]
struct Fake(Arc<Mutex<(VecDeque<Response>, Vec<Call>)>>);
impl Fake {
    fn bytes(&self, name: &str) {
        self.queue(Response::Bytes(fixture(name)));
    }
    fn json(&self, body: Value) {
        self.queue(Response::Bytes(serde_json::to_vec(&body).unwrap()));
    }
    fn queue(&self, r: Response) {
        self.0.lock().unwrap().0.push_back(r);
    }
    fn count(&self) -> usize {
        self.0.lock().unwrap().1.len()
    }
}
impl OutboundTransport for Fake {
    fn send<'a>(
        &'a self,
        peer: &'a AuthenticatedPeerId,
        endpoint: Endpoint,
        body: Vec<u8>,
        limits: RequestLimits,
    ) -> TransportFuture<'a> {
        Box::pin(async move {
            let cancel = limits.cancel.clone();
            let response = {
                let mut s = self.0.lock().unwrap();
                s.1.push(Call {
                    endpoint,
                    peer: peer.as_str().into(),
                    body: serde_json::from_slice(&body).unwrap(),
                    limits,
                });
                s.0.pop_front()
                    .expect("No automatic request/retry expected")
            };
            match response {
                Response::Bytes(v) => Ok(v),
                Response::Error(e) => Err(e),
                Response::Wait => {
                    cancel.cancelled().await;
                    Err(TransportError::Cancelled)
                }
            }
        })
    }
}
fn client(fake: &Fake) -> ObserverClient<Fake> {
    ObserverClient::new(
        AuthenticatedPeerId::new("verified-peer").unwrap(),
        id(2),
        fake.clone(),
    )
}
fn connected(fake: &Fake) -> ObserverClient<Fake> {
    let client = client(fake);
    fake.bytes("discovery");
    assert!(matches!(
        block_on(client.discover(FallbackPolicy::Never)).unwrap(),
        DiscoveryOutcome::Available(_)
    ));
    fake.bytes("pair-selection");
    block_on(client.pair(id(3))).unwrap();
    client
}
fn start<F: Future>(f: Pin<&mut F>) {
    assert!(matches!(
        f.poll(&mut Context::from_waker(noop_waker_ref())),
        Poll::Pending
    ));
}
#[test]
fn explicit_v2_client_uses_canonical_requests_without_automatic_acks() {
    let fake = Fake::default();
    let client = connected(&fake);
    let binding = client.binding().unwrap();
    fake.bytes("snapshot");
    let request = wire::GetSnapshot::from(GetSnapshot {});
    let wire::Reply::Success(snapshot) = block_on(client.execute(&request)).unwrap() else {
        panic!()
    };
    assert_eq!(snapshot.entries.len(), 3);
    fake.bytes("events");
    block_on(client.poll()).unwrap();
    let calls = &fake.0.lock().unwrap().1;
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[0].endpoint, Endpoint::DiscoverV2);
    assert_eq!(calls[0].body, json!({}));
    assert_eq!(calls[1].endpoint, Endpoint::PairV2);
    assert_eq!(calls[1].body, value("pair-offer"));
    assert_eq!(calls[2].body, value("request-snapshot"));
    assert_eq!(calls[3].body, value("events-request"));
    for call in calls {
        assert_eq!(call.peer, "verified-peer");
        assert_eq!(call.limits.max_request_bytes, MAX_REQUEST_BYTES);
        assert_eq!(call.limits.max_response_bytes, MAX_RESPONSE_BYTES);
    }
    assert_eq!(binding.connection_id, id(4));
}
#[test]
fn default_client_remains_v1_and_does_not_discover_or_offer_observer() {
    let fake = Fake::default();
    fake.json(json!({"ok":true,"value":{"paired":true,"terminalControl":"exclusive-v1"}}));
    let client = shellfox_client::Client::new(
        AuthenticatedPeerId::new("verified-peer").unwrap(),
        ClientId::new_v4(),
        fake.clone(),
    );
    block_on(client.pair()).unwrap();
    let s = fake.0.lock().unwrap();
    assert_eq!(s.1.len(), 1);
    assert_eq!(s.1[0].endpoint, Endpoint::Pair);
    assert!(s.1[0].body.get("version").is_none());
}
#[test]
fn profile_denied_input_layout_and_upload_never_reach_transport() {
    let fake = Fake::default();
    let client = connected(&fake);
    let identity = json!({"tabId":id(5),"generation":id(6),"controlEpoch":1});
    for (method, mut payload) in [
        ("writeTerminal", identity.clone()),
        ("resizeTerminal", identity.clone()),
        (
            "takeTerminalControl",
            json!({"tabId":id(5),"generation":id(6)}),
        ),
        ("beginTerminalUpload", identity.clone()),
        ("writeTerminalUpload", identity.clone()),
        ("finishTerminalUpload", identity.clone()),
        ("cancelTerminalUpload", identity.clone()),
    ] {
        match method {
            "writeTerminal" => payload["data"] = json!("\0"),
            "resizeTerminal" => {
                payload["cols"] = json!(80);
                payload["rows"] = json!(24);
            }
            "beginTerminalUpload" => {
                payload["fileName"] = json!("file");
                payload["size"] = json!(0);
            }
            "writeTerminalUpload" => {
                payload["uploadId"] = json!(id(7));
                payload["offset"] = json!(0);
                payload["data"] = json!("AQ==");
            }
            "finishTerminalUpload" | "cancelTerminalUpload" => payload["uploadId"] = json!(id(7)),
            _ => {}
        }
        let cmd: wire::Command =
            serde_json::from_value(json!({"method":method,"payload":payload})).unwrap();
        assert!(
            matches!(
                block_on(client.execute_command(&cmd)),
                Err(ClientError::ProfileDenied)
            ),
            "{method}"
        );
    }
    assert_eq!(fake.count(), 2);
}
#[test]
fn v2_settings_response_omission_reaches_presentation_without_v1_defaults() {
    let fake = Fake::default();
    let client = connected(&fake);
    let all: Vec<Value> = serde_json::from_slice(&fixture("methods")).unwrap();
    let case = all
        .into_iter()
        .find(|v| v["method"] == "setTerminalEnv")
        .unwrap();
    assert!(case["response"]["value"].get("pythonPath").is_none());
    fake.json(case["response"].clone());
    let command =
        wire::Command::SetTerminalEnv(wire::SetTerminalEnv::from(SetTerminalEnv { env: vec![] }));
    let wire::Reply::Success(value) = block_on(client.execute_command(&command)).unwrap() else {
        panic!()
    };
    assert!(value.get("pythonPath").is_none());
    assert_eq!(
        value.as_object().unwrap().keys().collect::<std::collections::BTreeSet<_>>(),
        case["response"]["value"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<std::collections::BTreeSet<_>>()
    );
}
#[test]
fn close_tab_is_admissible_manager_lifecycle_not_a_delivery_grant() {
    let fake = Fake::default();
    let client = connected(&fake);
    let all: Vec<Value> = serde_json::from_slice(&fixture("methods")).unwrap();
    let case = all.into_iter().find(|v| v["method"] == "closeTab").unwrap();
    let command: wire::Command =
        serde_json::from_value(json!({"method":"closeTab","payload":case["request"]["payload"]}))
            .unwrap();
    assert!(command.is_profile_allowed());
    fake.json(case["response"].clone());
    assert!(matches!(
        block_on(client.execute_command(&command)).unwrap(),
        wire::Reply::Success(_)
    ));
    let calls = &fake.0.lock().unwrap().1;
    assert_eq!(calls[2].body, case["request"]);
    assert!(calls[2].body["payload"].get("delivery").is_none());
    // Actual manager permission and source stop/drain are backend checks, not this fixture.
}
#[test]
fn mixed_catalog_coverage_failure_does_not_poison_independent_normal_operation() {
    let fake = Fake::default();
    let client = connected(&fake);
    let binding = serde_json::to_value(client.binding().unwrap()).unwrap();
    fake.json(json!({"version":2,"binding":binding,"ok":false,"error":{"code":"UNSUPPORTED","message":"Coverage is unavailable.","retryable":false,"detail":{"kind":"coverageUnsupported","issues":[{"reason":"evidenceUnavailable","fields":["createdAt"]}],"issueCount":1}}}));
    assert!(matches!(
        block_on(client.execute(&wire::GetSnapshot::from(GetSnapshot {}))).unwrap(),
        wire::Reply::Failure(_)
    ));
    assert_eq!(client.status(), Status::Connected);
    fake.json(json!({"version":2,"binding":binding,"ok":true,"value":{"cwd":"/fixture"}}));
    assert!(matches!(
        block_on(client.execute(&wire::GetHomeDirectory::from(GetHomeDirectory {}))).unwrap(),
        wire::Reply::Success(_)
    ));
    assert_eq!(fake.count(), 4);
}
