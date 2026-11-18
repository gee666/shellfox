#![cfg(feature = "local-host")]
use futures::{executor::block_on, task::noop_waker_ref};
use shellfox_client::{protocol::*, *};
use std::{
    future::Future,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Experiment {
    operation: String,
}
impl Validate for Experiment {
    fn validate(&self) -> Result<(), ValidationError> {
        validation::ensure(self.operation == "snapshot")
    }
}
impl LocalRequest for Experiment {
    type Response = Disconnected;
    const METHOD: &'static str = LOCAL_NAMESPACE;
}
#[derive(serde::Serialize, serde::Deserialize)]
struct NotExperiment {}
impl Validate for NotExperiment {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
impl LocalRequest for NotExperiment {
    type Response = Disconnected;
    const METHOD: &'static str = "saveSettings";
}
type Calls = Arc<Mutex<Vec<(Vec<u8>, RequestLimits)>>>;
#[derive(Clone, Default)]
struct LocalFake {
    calls: Calls,
    hold: bool,
}
impl LocalTransport for LocalFake {
    fn send<'a>(&'a self, body: Vec<u8>, limits: RequestLimits) -> TransportFuture<'a> {
        Box::pin(async move {
            self.calls.lock().unwrap().push((body, limits));
            if self.hold {
                std::future::pending().await
            } else {
                Ok(br#"{"ok":true,"value":{"disconnected":true}}"#.to_vec())
            }
        })
    }
}
#[test]
fn local_experiment_uses_typed_namespace_without_remote_peer_or_fake_pair() {
    let fake = LocalFake::default();
    let id = ClientId::new_v4();
    let client = LocalClient::new(id, fake.clone());
    let reply = block_on(client.execute(&Experiment {
        operation: "snapshot".into(),
    }))
    .unwrap();
    assert!(matches!(
        reply,
        Reply::Success(Disconnected { disconnected: true })
    ));
    let calls = fake.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let json: serde_json::Value = serde_json::from_slice(&calls[0].0).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"clientId":id,"method":"experimental.rust.v1","payload":{"operation":"snapshot"}})
    );
    assert_eq!(calls[0].1.max_request_bytes, MAX_REQUEST_BYTES);
    assert_eq!(calls[0].1.max_response_bytes, MAX_RESPONSE_BYTES);
}
#[test]
fn unsealed_local_request_cannot_select_manager_or_remote_namespace() {
    let fake = LocalFake::default();
    let client = LocalClient::new(ClientId::new_v4(), fake.clone());
    assert!(matches!(
        block_on(client.execute(&NotExperiment {})),
        Err(ClientError::Validation(_))
    ));
    assert!(fake.calls.lock().unwrap().is_empty());
}
#[test]
fn dropping_local_future_cancels_native_frame() {
    let fake = LocalFake {
        hold: true,
        ..LocalFake::default()
    };
    let client = LocalClient::new(ClientId::new_v4(), fake.clone());
    let input = Experiment {
        operation: "snapshot".into(),
    };
    let mut future = Box::pin(client.execute(&input));
    assert!(matches!(
        future
            .as_mut()
            .poll(&mut Context::from_waker(noop_waker_ref())),
        Poll::Pending
    ));
    let token = fake.calls.lock().unwrap()[0].1.cancel.clone();
    assert!(!token.is_cancelled());
    drop(future);
    assert!(token.is_cancelled());
}
#[test]
fn explicit_local_cancellation_does_not_send_or_retry() {
    let fake = LocalFake::default();
    let client = LocalClient::new(ClientId::new_v4(), fake.clone());
    let cancel = CancellationToken::new();
    cancel.cancel();
    let input = Experiment {
        operation: "snapshot".into(),
    };
    assert!(matches!(
        block_on(client.execute_with_cancel(&input, cancel)),
        Err(ClientError::Transport(TransportError::Cancelled))
    ));
    assert!(fake.calls.lock().unwrap().is_empty());
}
