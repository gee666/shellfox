#![cfg(feature = "local-host")]
use futures::{executor::block_on, task::noop_waker_ref};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use shellfox_client::{
    protocol::{automation::*, *},
    *,
};
use std::{
    collections::BTreeMap,
    future::Future,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};

type Calls = Arc<Mutex<Vec<(Value, RequestLimits)>>>;
#[derive(Clone, Default)]
struct AutomationTransport {
    calls: Calls,
    hold: bool,
}
impl LocalTransport for AutomationTransport {
    fn send<'a>(&'a self, body: Vec<u8>, limits: RequestLimits) -> TransportFuture<'a> {
        Box::pin(async move {
            let request: Value = serde_json::from_slice(&body).unwrap();
            let method = request["method"].as_str().unwrap().to_owned();
            self.calls.lock().unwrap().push((request, limits));
            if self.hold {
                return std::future::pending().await;
            }
            let run = run();
            let value = match method.as_str() {
                "automation.start" | "automation.show" => run,
                "automation.list" => json!([run]),
                "automation.read" => {
                    json!({"frames":[{"sequence":1,"stream":"stdout","data":"AQ=="}],"run":run})
                }
                "automation.write" => json!({"accepted":true}),
                "automation.stop" => json!({"stopped":true}),
                _ => panic!("Unexpected local method sent"),
            };
            Ok(serde_json::to_vec(&json!({"ok":true,"value":value})).unwrap())
        })
    }
}
fn run() -> Value {
    json!({"id":ClientId::nil(),"pid":42,"executable":"fixture-only-never-run","cwd":"/fixture",
        "title":"test","state":"running","exitCode":null,"signal":null,"sessionId":null,"tabId":null})
}
fn start() -> AutomationStart {
    AutomationStart {
        request_id: ClientId::nil(),
        executable: "fixture-only-never-run".into(),
        args: vec!["argument".into()],
        cwd: "/fixture".into(),
        env: BTreeMap::new(),
        title: None,
        detached: false,
    }
}
fn accepted<R: LocalRequest>(
    client: &LocalClient<AutomationTransport>,
    request: &R,
) -> R::Response {
    let Reply::Success(value) = block_on(client.execute(request)).unwrap() else {
        panic!("Local request was rejected");
    };
    value
}
#[test]
fn all_six_real_typed_automation_contracts_are_accepted_with_existing_limits() {
    let transport = AutomationTransport::default();
    let id = ClientId::new_v4();
    let client = LocalClient::new(id, transport.clone());
    let before = Instant::now();
    assert_eq!(accepted(&client, &start()).pid, 42);
    assert_eq!(accepted(&client, &AutomationList {}).0.len(), 1);
    assert_eq!(
        accepted(
            &client,
            &AutomationRead {
                id: ClientId::nil(),
                after: 0
            }
        )
        .frames
        .len(),
        1
    );
    assert!(
        accepted(
            &client,
            &AutomationWrite {
                id: ClientId::nil(),
                data: "AQ==".into(),
                eof: true
            }
        )
        .accepted
    );
    assert!(
        accepted(
            &client,
            &AutomationStop {
                id: ClientId::nil()
            }
        )
        .stopped
    );
    assert_eq!(
        accepted(
            &client,
            &AutomationShow {
                id: ClientId::nil()
            }
        )
        .pid,
        42
    );
    let calls = transport.calls.lock().unwrap();
    assert_eq!(calls.len(), 6);
    let methods: Vec<_> = calls
        .iter()
        .map(|(body, _)| body["method"].as_str().unwrap())
        .collect();
    assert_eq!(
        methods,
        [
            "automation.start",
            "automation.list",
            "automation.read",
            "automation.write",
            "automation.stop",
            "automation.show"
        ]
    );
    for (body, limits) in calls.iter() {
        assert_eq!(body["clientId"], id.to_string());
        assert_eq!(limits.max_request_bytes, MAX_REQUEST_BYTES);
        assert_eq!(limits.max_response_bytes, MAX_RESPONSE_BYTES);
        assert!(
            limits.deadline >= before
                && limits.deadline <= Instant::now() + Duration::from_secs(30)
        );
    }
    assert!(calls[0].0["payload"].get("title").is_none());
}

#[derive(Serialize, Deserialize)]
struct Unknown<const N: u8> {}
impl<const N: u8> Validate for Unknown<N> {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}
impl<const N: u8> LocalRequest for Unknown<N> {
    type Response = AutomationAccepted;
    const METHOD: &'static str = match N {
        0 => "automation",
        1 => "automation.",
        2 => "automation.delete",
        3 => "automation.start.extra",
        4 => "automation.Start",
        5 => "automation/start",
        6 => "getSnapshot",
        7 => "saveSettings",
        8 => "host.stop",
        _ => "private.local.v2",
    };
}
fn rejected<const N: u8>(client: &LocalClient<AutomationTransport>) {
    assert!(matches!(
        block_on(client.execute(&Unknown::<N> {})),
        Err(ClientError::Validation(_))
    ));
}
#[test]
fn only_the_six_exact_automation_names_are_allowed_not_prefixes_or_manager_privileges() {
    let transport = AutomationTransport::default();
    let client = LocalClient::new(ClientId::new_v4(), transport.clone());
    rejected::<0>(&client);
    rejected::<1>(&client);
    rejected::<2>(&client);
    rejected::<3>(&client);
    rejected::<4>(&client);
    rejected::<5>(&client);
    rejected::<6>(&client);
    rejected::<7>(&client);
    rejected::<8>(&client);
    rejected::<9>(&client);
    assert!(transport.calls.lock().unwrap().is_empty());
}
#[test]
fn automation_uses_unchanged_validation_and_explicit_cancel_without_sending_or_retrying() {
    let transport = AutomationTransport::default();
    let client = LocalClient::new(ClientId::new_v4(), transport.clone());
    let invalid = AutomationWrite {
        id: ClientId::nil(),
        data: "AB==".into(),
        eof: false,
    };
    assert!(matches!(
        block_on(client.execute(&invalid)),
        Err(ClientError::Validation(_))
    ));
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
        block_on(client.execute_with_cancel(&AutomationList {}, cancel)),
        Err(ClientError::Transport(TransportError::Cancelled))
    ));
    assert!(transport.calls.lock().unwrap().is_empty());
}
#[test]
fn dropping_an_automation_call_cancels_only_its_transport_work() {
    let transport = AutomationTransport {
        hold: true,
        ..AutomationTransport::default()
    };
    let client = LocalClient::new(ClientId::new_v4(), transport.clone());
    let request = AutomationList {};
    let mut work = Box::pin(client.execute(&request));
    assert!(matches!(
        work.as_mut()
            .poll(&mut Context::from_waker(noop_waker_ref())),
        Poll::Pending
    ));
    let token = transport.calls.lock().unwrap()[0].1.cancel.clone();
    assert!(!token.is_cancelled());
    drop(work);
    assert!(token.is_cancelled());
    assert_eq!(transport.calls.lock().unwrap().len(), 1);
}
#[test]
fn automation_retains_encoded_request_and_response_caps() {
    let transport = AutomationTransport::default();
    let client = LocalClient::new(ClientId::new_v4(), transport.clone());
    let mut input = start();
    input.args = vec!["x".repeat(32768); 32];
    input.validate().unwrap();
    assert!(matches!(
        block_on(client.execute(&input)),
        Err(ClientError::Validation(_))
    ));
    assert!(transport.calls.lock().unwrap().is_empty());
    struct OversizedResponse;
    impl LocalTransport for OversizedResponse {
        fn send<'a>(&'a self, _body: Vec<u8>, _limits: RequestLimits) -> TransportFuture<'a> {
            Box::pin(async { Ok(vec![b' '; MAX_RESPONSE_BYTES + 1]) })
        }
    }
    let client = LocalClient::new(ClientId::new_v4(), OversizedResponse);
    assert!(matches!(
        block_on(client.execute(&AutomationList {})),
        Err(ClientError::Validation(_))
    ));
}
#[test]
fn automation_contracts_remain_impossible_to_select_through_remote_commands() {
    assert_eq!(REMOTE_METHODS.len(), 43);
    for method in [
        "automation.start",
        "automation.list",
        "automation.read",
        "automation.write",
        "automation.stop",
        "automation.show",
    ] {
        assert!(!REMOTE_METHODS.contains(&method));
        assert!(
            serde_json::from_value::<RemoteCommand>(json!({"method":method,"payload":{}})).is_err()
        );
    }
}
