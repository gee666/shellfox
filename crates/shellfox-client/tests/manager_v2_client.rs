use futures::{executor::block_on, task::noop_waker_ref};
#[path = "support/manager_native_recovery.rs"]
mod native_recovery;
use serde_json::{json, Value};
use shellfox_client::manager_v2::{
    ClientError, DiscoveryOutcome, FallbackPolicy, ManagerClient, Status,
};
use shellfox_protocol::{manager_v2 as m, *};
use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../shellfox-protocol/tests/fixtures/manager_v2/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}
fn value(name: &str) -> Value {
    serde_json::from_slice(&fixture(name)).unwrap()
}
fn id(n: u8) -> m::Id {
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
    body: Value,
    cancel: CancellationToken,
}
#[derive(Clone, Default)]
struct Fake(Arc<Mutex<(VecDeque<Response>, Vec<Call>)>>);
impl Fake {
    fn bytes(&self, name: &str) {
        self.0
            .lock()
            .unwrap()
            .0
            .push_back(Response::Bytes(fixture(name)));
    }
    fn json(&self, v: Value) {
        self.0
            .lock()
            .unwrap()
            .0
            .push_back(Response::Bytes(serde_json::to_vec(&v).unwrap()));
    }
    fn error(&self, e: TransportError) {
        self.0.lock().unwrap().0.push_back(Response::Error(e));
    }
    fn wait(&self) {
        self.0.lock().unwrap().0.push_back(Response::Wait);
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
        assert_eq!(peer.as_str(), "verified-peer");
        Box::pin(async move {
            let cancel = limits.cancel;
            let response = {
                let mut s = self.0.lock().unwrap();
                s.1.push(Call {
                    endpoint,
                    body: serde_json::from_slice(&body).unwrap(),
                    cancel: cancel.clone(),
                });
                s.0.pop_front()
            }
            .expect("No automatic network retry/claim expected");
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
fn make(fake: &Fake) -> ManagerClient<Fake> {
    ManagerClient::new(
        AuthenticatedPeerId::new("verified-peer").unwrap(),
        id(2),
        fake.clone(),
    )
}
fn connected(fake: &Fake) -> ManagerClient<Fake> {
    let client = make(fake);
    fake.bytes("discovery");
    block_on(client.discover(FallbackPolicy::Never)).unwrap();
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
fn manager_selection_never_claims_or_changes_observer_profile() {
    let fake = Fake::default();
    let client = connected(&fake);
    assert_eq!(fake.count(), 2);
    assert_eq!(client.status(), Status::Connected);
    let calls = &fake.0.lock().unwrap().1;
    assert_eq!(calls[0].endpoint, Endpoint::DiscoverV2);
    assert_eq!(calls[1].body, value("pair-offer"));
    assert_eq!(client.binding().unwrap().profile, m::Profile::Manager);
    assert!(shellfox_protocol::decode::<
        shellfox_protocol::observer_v2::Reply<shellfox_protocol::observer_v2::PairSelection>,
    >(&fixture("pair-selection"))
    .is_err());
}
#[test]
fn all43_canonical_methods_use_typed_manager_responses_and_refs() {
    let fake = Fake::default();
    let client = connected(&fake);
    let cases: Vec<Value> = serde_json::from_slice(&fixture("methods")).unwrap();
    for case in cases {
        let command: m::Command = serde_json::from_value(
            json!({"method":case["method"],"payload":case["request"]["payload"]}),
        )
        .unwrap();
        fake.json(case["response"].clone());
        assert!(matches!(
            block_on(client.execute_command(&command)).unwrap(),
            m::Reply::Success(_)
        ));
        assert_eq!(
            fake.0.lock().unwrap().1.last().unwrap().body,
            case["request"]
        );
    }
    assert_eq!(fake.count(), 45);
}
#[test]
fn binary_input_requires_matching_actual_receipt_not_requested_length() {
    let fake = Fake::default();
    let client = connected(&fake);
    let request = m::decode_request(&fixture("request-write-bytes")).unwrap();
    let m::Command::WriteTerminal(request) = request.command else {
        panic!()
    };
    fake.bytes("accepted-bytes");
    let m::Reply::Success(receipt) = block_on(client.execute(&request)).unwrap() else {
        panic!()
    };
    assert_eq!(receipt.accepted_bytes, 4);
    let mut bad = value("accepted-bytes");
    bad["value"]["acceptedBytes"] = json!(3);
    fake.json(bad);
    assert!(matches!(
        block_on(client.execute(&request)),
        Err(ClientError::InvalidResponse)
    ));
    assert!(client.binding().is_none());
    assert_eq!(fake.count(), 4);
}
#[test]
fn valid_paste_can_fail_encoded_budget_without_fake_receipt_or_retry() {
    let fake = Fake::default();
    let client = connected(&fake);
    let decoded = m::decode_request(&fixture("request-write-paste")).unwrap();
    let m::Command::WriteTerminal(mut request) = decoded.command else {
        panic!()
    };
    request.input = m::Input::Paste {
        text: "x".repeat(m::MAX_INPUT_PAYLOAD_BYTES),
    };
    request.validate().unwrap();
    fake.bytes("encoded-input-limit");
    let m::Reply::Failure(error) = block_on(client.execute(&request)).unwrap() else {
        panic!("An overflow is not an accepted/zero-byte receipt")
    };
    assert!(matches!(
        error.detail,
        m::ErrorDetail::ResourceLimit {
            limit: m::Limit::EncodedInputBytes
        }
    ));
    assert_eq!(client.status(), Status::Connected);
    assert_eq!(fake.count(), 3);
    assert_eq!(request.hints.settled_at, Some(None));
}
#[test]
fn general_manager_never_falls_back_to_observer_interactive() {
    let fake = Fake::default();
    let client = make(&fake);
    let mut disco = value("discovery");
    disco["value"]["profiles"] = json!(["observer-v2"]);
    fake.json(disco);
    assert!(matches!(
        block_on(client.discover(FallbackPolicy::Never)).unwrap(),
        DiscoveryOutcome::Incompatible(_)
    ));
    assert!(matches!(
        block_on(client.pair(id(3))),
        Err(ClientError::NotDiscovered)
    ));
    assert_eq!(fake.count(), 1);
    let fake = Fake::default();
    let client = make(&fake);
    fake.error(TransportError::Rejected { status: 404 });
    fake.json(json!({"ok":true,"value":{"service":"shellfox","protocolVersion":1}}));
    assert!(matches!(
        block_on(client.discover(FallbackPolicy::OfferLegacy)).unwrap(),
        DiscoveryOutcome::LegacyAvailable
    ));
    assert_eq!(fake.count(), 2);
    assert_eq!(fake.0.lock().unwrap().1[1].endpoint, Endpoint::Discover);
    assert!(client.binding().is_none());
}
#[test]
fn local_cancel_and_drop_release_shared_lifecycle_permits_without_repair() {
    let fake = Fake::default();
    let client = connected(&fake);
    let binding = client.binding();
    let request = m::GetSnapshot::from(GetSnapshot {});
    let mut pending = Vec::new();
    for _ in 0..8 {
        fake.wait();
        let mut f = Box::pin(client.execute(&request));
        start(f.as_mut());
        pending.push(f);
    }
    assert!(matches!(
        block_on(client.execute(&request)),
        Err(ClientError::Busy)
    ));
    drop(pending);
    fake.error(TransportError::Cancelled);
    assert!(matches!(
        block_on(client.poll()),
        Err(ClientError::Transport(TransportError::Cancelled))
    ));
    assert_eq!(client.binding(), binding);
    assert_eq!(client.status(), Status::Connected);
    assert!(fake.0.lock().unwrap().1[2..10]
        .iter()
        .all(|c| c.cancel.is_cancelled()));
}
#[test]
fn uncertain_write_or_takeover_is_not_replayed_and_old_binding_is_stale() {
    for method in ["request-write-bytes", "request-takeover"] {
        let fake = Fake::default();
        let client = connected(&fake);
        let envelope = value(method);
        let command: m::Command = serde_json::from_value(
            json!({"method":envelope["method"],"payload":envelope["payload"]}),
        )
        .unwrap();
        fake.error(TransportError::Deadline);
        assert!(matches!(
            block_on(client.execute_command(&command)),
            Err(ClientError::Transport(TransportError::Deadline))
        ));
        assert_eq!(fake.count(), 3);
        assert!(client.binding().is_none());
    }
    let fake = Fake::default();
    let client = connected(&fake);
    let request = m::GetSnapshot::from(GetSnapshot {});
    fake.wait();
    let mut work = Box::pin(client.execute(&request));
    start(work.as_mut());
    client.reconnect(id(9));
    assert!(matches!(block_on(work), Err(ClientError::StaleConnection)));
}
