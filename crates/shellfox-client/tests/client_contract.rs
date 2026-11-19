#[path = "support/client_cancellation.rs"]
mod cancellation;

use futures::{executor::block_on, task::noop_waker_ref};
use shellfox_client::{protocol::*, *};
use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

const PAIR: &[u8] = include_bytes!("fixtures/remote-pair-success.json");
const SNAPSHOT: &[u8] = include_bytes!("fixtures/manager-snapshot-success.json");
const OWNED: &[u8] = include_bytes!("fixtures/terminal-attach-owned-success.json");
const EVENTS: &[u8] = include_bytes!("fixtures/terminal-events-success.json");
const ID: &str = "10000000-0000-4000-8000-000000000001";
enum Response {
    Bytes(Vec<u8>),
    Error(TransportError),
    Hold,
}
struct Call {
    endpoint: Endpoint,
    body: serde_json::Value,
    limits: RequestLimits,
}
#[derive(Clone, Default)]
struct Fake(Arc<Mutex<(VecDeque<Response>, Vec<Call>)>>);
impl Fake {
    fn bytes(&self, bytes: &[u8]) {
        self.0
            .lock()
            .unwrap()
            .0
            .push_back(Response::Bytes(bytes.into()));
    }
    fn queue(&self, response: Response) {
        self.0.lock().unwrap().0.push_back(response);
    }
    fn calls(&self) -> usize {
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
        assert_eq!(peer.as_str(), "host-1");
        Box::pin(async move {
            let cancel = limits.cancel.clone();
            let response = {
                let mut inner = self.0.lock().unwrap();
                inner.1.push(Call {
                    endpoint,
                    body: serde_json::from_slice(&body).unwrap(),
                    limits,
                });
                inner.0.pop_front().expect("unexpected automatic retry")
            };
            match response {
                Response::Bytes(bytes) => Ok(bytes),
                Response::Error(error) => Err(error),
                Response::Hold => {
                    cancel.cancelled().await;
                    Err(TransportError::Cancelled)
                }
            }
        })
    }
}
fn client(fake: Fake) -> Client<Fake> {
    Client::new(
        AuthenticatedPeerId::new("host-1").unwrap(),
        ID.parse().unwrap(),
        fake,
    )
}
fn connected(fake: &Fake) -> Client<Fake> {
    fake.bytes(PAIR);
    let client = client(fake.clone());
    block_on(client.pair()).unwrap();
    client
}
fn start<F: Future>(future: Pin<&mut F>) {
    assert!(matches!(
        future.poll(&mut Context::from_waker(noop_waker_ref())),
        Poll::Pending
    ));
}

#[test]
fn typed_fake_host_snapshot_and_terminal_poll_preserve_existing_wire() {
    let fake = Fake::default();
    let client = connected(&fake);
    fake.bytes(SNAPSHOT);
    let Reply::Success(snapshot) = block_on(client.execute(&GetSnapshot {})).unwrap() else {
        panic!()
    };
    assert!(!snapshot.sessions.is_empty());
    let attachment: Reply<TerminalAttachment> = decode(OWNED).unwrap();
    let Reply::Success(TerminalAttachment::Owned(a)) = attachment else {
        panic!()
    };
    fake.bytes(OWNED);
    block_on(client.execute(&AttachTerminal {
        tab_id: a.tab_id,
        generation: None,
        after_sequence: None,
    }))
    .unwrap();
    fake.bytes(EVENTS);
    block_on(client.poll()).unwrap();
    assert_eq!(client.recovery_format(), Some(RecoveryFormat::AnsiV1));
    let calls = &fake.0.lock().unwrap().1;
    assert_eq!(calls.len(), 4); // Poll never sends an ACK.
    assert_eq!(calls[0].body, serde_json::json!({"clientId":ID}));
    assert_eq!(
        calls[1].body,
        serde_json::json!({"clientId":ID,"method":"getSnapshot","payload":{}})
    );
    assert_eq!(calls[1].endpoint, Endpoint::Request);
    assert_eq!(
        calls[2].body["payload"],
        serde_json::json!({"tabId":a.tab_id})
    );
    for call in calls {
        assert_eq!(call.limits.max_request_bytes, MAX_REQUEST_BYTES);
        assert_eq!(call.limits.max_response_bytes, MAX_RESPONSE_BYTES);
    }
}
#[test]
fn uncertain_mutation_is_never_retried() {
    let fake = Fake::default();
    let client = connected(&fake);
    fake.queue(Response::Error(TransportError::Deadline));
    let input = CreateSession {
        title: None,
        cwd: "/tmp".into(),
        request_id: ID.parse().unwrap(),
    };
    assert!(matches!(
        block_on(client.execute(&input)),
        Err(ClientError::Transport(TransportError::Deadline))
    ));
    assert_eq!(fake.calls(), 2);
    assert_eq!(client.state().status, ConnectionStatus::Reconnecting);
    assert!(matches!(
        block_on(client.execute(&input)),
        Err(ClientError::NotConnected)
    ));
    assert_eq!(fake.calls(), 2);
}
#[test]
fn pending_approval_and_denial_are_host_permissions_not_transport_authentication() {
    let fake = Fake::default();
    let client = client(fake.clone());
    fake.bytes(include_bytes!("fixtures/remote-approval-required.json"));
    assert!(matches!(
        block_on(client.pair()).unwrap(),
        Reply::Failure(_)
    ));
    assert_eq!(client.state().permission, Permission::AwaitingApproval);
    assert!(client.state().retry_after.is_some());
    fake.bytes(include_bytes!("fixtures/remote-access-denied.json"));
    block_on(client.pair()).unwrap();
    assert_eq!(client.state().status, ConnectionStatus::Rejected);
    assert_eq!(client.state().retry_after, None);
    assert_eq!(fake.calls(), 2);
}
#[test]
fn incompatible_pair_is_rejected_without_invented_capabilities() {
    let fake = Fake::default();
    let client = client(fake.clone());
    fake.bytes(br#"{"ok":true,"value":{"paired":true}}"#);
    assert!(matches!(
        block_on(client.pair()),
        Err(ClientError::Validation(_))
    ));
    assert_eq!(client.state().status, ConnectionStatus::Rejected);
    assert_eq!(client.state().retry_after, None);
    assert_eq!(client.recovery_format(), None);
    assert_eq!(fake.calls(), 1);
}
#[test]
fn invalid_mutation_never_reaches_transport() {
    let fake = Fake::default();
    let client = connected(&fake);
    let input = CreateSession {
        title: None,
        cwd: "https://invalid".into(),
        request_id: ID.parse().unwrap(),
    };
    assert!(matches!(
        block_on(client.execute(&input)),
        Err(ClientError::Validation(_))
    ));
    assert_eq!(fake.calls(), 1);
}
#[test]
fn disconnect_only_withdraws_view_no_shell_close_method() {
    let fake = Fake::default();
    let client = connected(&fake);
    fake.bytes(br#"{"ok":true,"value":{"disconnected":true}}"#);
    block_on(client.disconnect()).unwrap();
    let inner = fake.0.lock().unwrap();
    assert_eq!(inner.1[1].endpoint, Endpoint::Disconnect);
    assert_eq!(inner.1[1].body, serde_json::json!({"clientId":ID}));
    assert_eq!(client.state().status, ConnectionStatus::Disconnected);
}
#[test]
fn dropping_pair_future_cancels_native_call_and_releases_pair_flag() {
    let fake = Fake::default();
    let client = client(fake.clone());
    fake.queue(Response::Hold);
    let mut pending = Box::pin(client.pair());
    start(pending.as_mut());
    drop(pending);
    assert!(fake.0.lock().unwrap().1[0].limits.cancel.is_cancelled());
    assert_eq!(client.state().status, ConnectionStatus::Reconnecting);
    fake.bytes(PAIR);
    block_on(client.pair()).unwrap();
    assert_eq!(client.state().status, ConnectionStatus::Connected);
}
#[test]
fn dropping_poll_future_releases_poll_flag_and_only_cancels_that_call() {
    let fake = Fake::default();
    let client = connected(&fake);
    fake.queue(Response::Hold);
    let mut pending = Box::pin(client.poll());
    start(pending.as_mut());
    drop(pending);
    assert!(fake.0.lock().unwrap().1[1].limits.cancel.is_cancelled());
    fake.bytes(EVENTS);
    block_on(client.poll()).unwrap();
    assert_eq!(client.state().status, ConnectionStatus::Connected);
}
#[test]
fn dropping_execute_futures_releases_all_eight_permits() {
    let fake = Fake::default();
    let client = connected(&fake);
    let input = GetSnapshot {};
    let mut calls = Vec::new();
    for _ in 0..8 {
        fake.queue(Response::Hold);
        let mut future = Box::pin(client.execute(&input));
        start(future.as_mut());
        calls.push(future);
    }
    assert!(matches!(
        block_on(client.execute(&input)),
        Err(ClientError::TooManyRequests)
    ));
    drop(calls);
    for call in &fake.0.lock().unwrap().1[1..] {
        assert!(call.limits.cancel.is_cancelled());
    }
    fake.bytes(SNAPSHOT);
    assert!(matches!(
        block_on(client.execute(&input)).unwrap(),
        Reply::Success(_)
    ));
    assert_eq!(fake.calls(), 10);
}
#[test]
fn filesystem_denial_keeps_the_view_but_auth_failure_retires_it() {
    let fake = Fake::default();
    let client = connected(&fake);
    let connection = client.state().connection_id;
    let request = BeginFileWrite {
        session_id: None,
        tab_id: None,
        path: "/fixture/upload.bin".into(),
        size: 0,
        mode: FileWriteMode::Create,
        revision: None,
        content: FileContent::File,
        mime_type: None,
    };
    let fail = |code| {
        serde_json::to_vec(&Reply::<FileWriteTransfer>::Failure(AppError {
            code,
            message: "fixture file denial".into(),
            retryable: false,
        }))
        .unwrap()
    };
    fake.bytes(&fail(ErrorCode::AccessDenied));
    assert!(matches!(
        block_on(client.execute_with_connection(connection, &request)).unwrap(),
        Reply::Failure(_)
    ));
    assert_eq!(client.state().status, ConnectionStatus::Connected);
    assert!(client.with_connection(connection, || ()).is_ok());
    fake.bytes(&fail(ErrorCode::AuthFailed));
    assert!(matches!(
        block_on(client.execute_with_connection(connection, &request)).unwrap(),
        Reply::Failure(_)
    ));
    assert!(matches!(
        client.with_connection(connection, || ()),
        Err(ClientError::StaleConnection)
    ));
}

#[test]
fn reconnect_cancels_old_cycle_and_late_response_cannot_apply() {
    let fake = Fake::default();
    let client = connected(&fake);
    fake.queue(Response::Hold);
    let request = GetSnapshot {};
    let mut pending = Box::pin(client.execute(&request));
    start(pending.as_mut());
    let old = client.state().connection_id;
    client.reconnect(ClientId::new_v4());
    assert!(fake.0.lock().unwrap().1[1].limits.cancel.is_cancelled());
    fake.bytes(PAIR);
    block_on(client.pair()).unwrap();
    assert!(matches!(
        block_on(pending),
        Err(ClientError::StaleConnection)
    ));
    let current = client.state().connection_id;
    assert!(current > old);
    let calls = fake.calls();
    assert!(matches!(
        block_on(client.execute_with_connection(old, &request)),
        Err(ClientError::StaleConnection)
    ));
    assert!(matches!(
        client.with_connection(old, || unreachable!("retired publication")),
        Err(ClientError::StaleConnection)
    ));
    assert_eq!(fake.calls(), calls);
    fake.bytes(SNAPSHOT);
    block_on(client.execute_with_connection(current, &request)).unwrap();
}
