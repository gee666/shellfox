#![cfg(feature = "local-host")]
use futures::{executor::block_on, task::noop_waker_ref};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use shellfox_client::{protocol::*, *};
use std::{
    future::Future,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

// Client namespace probe only. Georgia's full Discover/Submit/Receipt DTO and
// Walter's server validator remain the authority for desktop launch payloads.
#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "camelCase", deny_unknown_fields)]
enum Discover {
    Discover,
}
impl Validate for Discover {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
impl LocalRequest for Discover {
    type Response = Configured;
    const METHOD: &'static str = DESKTOP_LAUNCH_NAMESPACE;
}
type Calls = Arc<Mutex<Vec<(Value, RequestLimits)>>>;
#[derive(Clone, Default)]
struct Transport {
    calls: Calls,
    hold: bool,
}
impl LocalTransport for Transport {
    fn send<'a>(&'a self, body: Vec<u8>, limits: RequestLimits) -> TransportFuture<'a> {
        Box::pin(async move {
            self.calls
                .lock()
                .unwrap()
                .push((serde_json::from_slice(&body).unwrap(), limits));
            if self.hold {
                std::future::pending().await
            } else {
                Ok(br#"{"ok":true,"value":{"configured":true}}"#.to_vec())
            }
        })
    }
}
#[test]
fn exact_private_launch_namespace_is_accepted_without_remote_pairing_or_extra_fields() {
    assert_eq!(DESKTOP_LAUNCH_NAMESPACE, "experimental.desktop.launch.v1");
    let transport = Transport::default();
    let id = ClientId::new_v4();
    let client = LocalClient::new(id, transport.clone());
    assert!(matches!(
        block_on(client.execute(&Discover::Discover)).unwrap(),
        Reply::Success(Configured { configured: true })
    ));
    let calls = transport.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].0,
        json!({"clientId":id,"method":"experimental.desktop.launch.v1","payload":{"operation":"discover"}})
    );
    assert_eq!(calls[0].1.max_request_bytes, MAX_REQUEST_BYTES);
    assert_eq!(calls[0].1.max_response_bytes, MAX_RESPONSE_BYTES);
}
#[derive(Serialize, Deserialize)]
struct Unknown<const N: u8> {}
impl<const N: u8> Validate for Unknown<N> {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
impl<const N: u8> LocalRequest for Unknown<N> {
    type Response = Configured;
    const METHOD: &'static str = match N {
        0 => "experimental.desktop.launch",
        1 => "experimental.desktop.launch.v2",
        2 => "experimental.desktop.launch.v1.extra",
        3 => "experimental.desktop.register.v1",
        4 => "experimental.desktop.launch.v1.register",
        5 => "experimental.desktop.launch.v1.claim",
        6 => "experimental.desktop.launch.v1.ack",
        7 => "experimental.other.v1",
        8 => "experimental.",
        _ => "experimental.desktop.launch.V1",
    };
}
fn reject<const N: u8>(client: &LocalClient<Transport>) {
    assert!(matches!(
        block_on(client.execute(&Unknown::<N> {})),
        Err(ClientError::Validation(_))
    ));
}
#[test]
fn neither_experimental_prefixes_nor_native_trust_methods_are_added() {
    let transport = Transport::default();
    let client = LocalClient::new(ClientId::new_v4(), transport.clone());
    reject::<0>(&client);
    reject::<1>(&client);
    reject::<2>(&client);
    reject::<3>(&client);
    reject::<4>(&client);
    reject::<5>(&client);
    reject::<6>(&client);
    reject::<7>(&client);
    reject::<8>(&client);
    reject::<9>(&client);
    assert!(transport.calls.lock().unwrap().is_empty());
    assert_eq!(REMOTE_METHODS.len(), 43);
    assert!(!REMOTE_METHODS.contains(&DESKTOP_LAUNCH_NAMESPACE));
    assert!(serde_json::from_value::<RemoteCommand>(
        json!({"method":DESKTOP_LAUNCH_NAMESPACE,"payload":{"operation":"discover"}})
    )
    .is_err());
}
#[test]
fn desktop_discovery_retains_pre_cancel_and_drop_cancellation_without_retry() {
    let transport = Transport {
        hold: true,
        ..Transport::default()
    };
    let client = LocalClient::new(ClientId::new_v4(), transport.clone());
    let token = CancellationToken::new();
    token.cancel();
    assert!(matches!(
        block_on(client.execute_with_cancel(&Discover::Discover, token)),
        Err(ClientError::Transport(TransportError::Cancelled))
    ));
    assert!(transport.calls.lock().unwrap().is_empty());
    let mut request = Box::pin(client.execute(&Discover::Discover));
    assert!(matches!(
        request
            .as_mut()
            .poll(&mut Context::from_waker(noop_waker_ref())),
        Poll::Pending
    ));
    let cancel = transport.calls.lock().unwrap()[0].1.cancel.clone();
    assert!(!cancel.is_cancelled());
    drop(request);
    assert!(cancel.is_cancelled());
    assert_eq!(transport.calls.lock().unwrap().len(), 1);
}
