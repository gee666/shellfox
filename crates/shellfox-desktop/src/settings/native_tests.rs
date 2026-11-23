use super::{
    api::{self, SettingsServices},
    native_gateway::{Gateway, Reply},
    native_services::NativeServices,
};
use shellfox_service::{
    native::{NativeAction, NativeResponse},
    remote::native as host,
};
use std::sync::{Arc, Mutex};
enum Outcome {
    State(host::RemoteControlState),
    Denied(&'static str),
    WrongResponse,
}
struct Fixture {
    requests: Mutex<Vec<NativeAction>>,
    outcome: Outcome,
}
impl Gateway for Fixture {
    fn execute(&self, request: NativeAction) -> Reply<'_> {
        self.requests.lock().unwrap().push(request);
        Box::pin(async {
            match &self.outcome {
                Outcome::State(state) => Ok(NativeResponse::Remote(state.clone())),
                Outcome::Denied(message) => Err(anyhow::anyhow!(*message)),
                Outcome::WrongResponse => Ok(NativeResponse::Applied),
            }
        })
    }
}
fn state() -> host::RemoteControlState {
    host::RemoteControlState {
        enabled: true,
        hostname: "owned-fixture".into(),
        status: host::RemoteNetworkStatus::Starting,
        login_url: None,
        error: None,
        peers: vec![host::RemotePeer {
            id: "peer-1".into(),
            hostname: "peer fixture".into(),
            addresses: vec!["100.64.0.2".into()],
            online: true,
            remembered: true,
            host_confirmed: true,
            connection: host::RemoteConnection::Connecting,
            last_error: None,
            attempt_id: Some("00000000-0000-4000-8000-000000000001".parse().unwrap()),
        }],
        pending: vec![],
        approvals: vec![],
        attempts: vec![host::RemoteAttempt {
            id: "00000000-0000-4000-8000-000000000001".parse().unwrap(),
            kind: host::AttemptKind::Connect {
                peer_id: "peer-1".into(),
            },
        }],
        cleanup_retained: true,
        incoming_legacy_terminal_attachment: false,
        terminal_capability_reason: "Incoming legacy terminal attachment is unavailable.".into(),
    }
}
fn fixture(outcome: Outcome) -> (NativeServices, Arc<Fixture>) {
    let gateway = Arc::new(Fixture {
        requests: Mutex::new(Vec::new()),
        outcome,
    });
    (NativeServices::injected(gateway.clone()), gateway)
}
fn run(service: &NativeServices, request: api::Request) -> anyhow::Result<api::Response> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(service.request(request))
}
#[test]
fn source_configuration_is_bound_to_the_real_typed_native_owner_action() {
    let (service, gateway) = fixture(Outcome::State(state()));
    assert!(matches!(
        run(
            &service,
            api::Request::ConfigureRemote {
                enabled: false,
                hostname: "new-fixture".into()
            }
        )
        .unwrap(),
        api::Response::Remote(_)
    ));
    let requests = gateway.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(
        matches!(&requests[0],NativeAction::Remote(host::RemoteRequest::Configure{enabled:false,hostname})if hostname=="new-fixture")
    );
}
#[test]
fn denied_and_unavailable_remain_errors_not_fabricated_disabled_states() {
    for message in [
        "Native controller access denied.",
        "Remote module is unavailable.",
    ] {
        let (service, gateway) = fixture(Outcome::Denied(message));
        let result = run(&service, api::Request::RemoteState);
        assert_eq!(result.err().unwrap().to_string(), message);
        assert!(matches!(
            &gateway.requests.lock().unwrap()[0],
            NativeAction::Remote(host::RemoteRequest::State)
        ));
    }
}
#[test]
fn pending_connections_attempt_ids_and_capability_limits_are_preserved() {
    let (service, _) = fixture(Outcome::State(state()));
    let api::Response::Remote(state) = run(&service, api::Request::RemoteState).unwrap() else {
        panic!("Expected remote state")
    };
    assert!(state.enabled);
    assert!(matches!(state.status, api::NetworkStatus::Starting));
    assert!(matches!(
        state.peers[0].connection,
        api::Connection::Connecting
    ));
    assert_eq!(state.attempts[0].id, "00000000-0000-4000-8000-000000000001");
    assert!(
        matches!(&state.attempts[0].kind,api::RemoteAttemptKind::Connect{peer_id}if peer_id=="peer-1")
    );
    assert_eq!(state.limitations.len(), 2);
    assert!(state.error.is_none());
    assert!(state.login_url.is_none());
}
#[test]
fn one_click_approval_sends_only_peer_id_to_the_fixed_server_policy() {
    let (service, gateway) = fixture(Outcome::State(state()));
    assert!(run(&service, api::Request::Approve("invalid\npeer".into())).is_err());
    assert!(gateway.requests.lock().unwrap().is_empty());
    assert!(matches!(
        run(&service, api::Request::Approve("peer-1".into())).unwrap(),
        api::Response::Remote(_)
    ));
    let requests = gateway.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(
        matches!(&requests[0],NativeAction::Remote(host::RemoteRequest::Approve{peer_id})if peer_id=="peer-1")
    );
}
#[test]
fn cancellation_binds_exact_attempt_ids_and_reject_invalid_ids_before_rpc() {
    let (service, gateway) = fixture(Outcome::State(state()));
    let id = "00000000-0000-4000-8000-000000000001";
    for bad in ["not-an-id", "00000000-0000-0000-0000-000000000000"] {
        assert!(run(&service, api::Request::CancelAttempt(bad.into())).is_err());
    }
    assert!(gateway.requests.lock().unwrap().is_empty());
    run(&service, api::Request::CancelAttempt(id.into())).unwrap();
    let requests = gateway.requests.lock().unwrap();
    assert!(
        matches!(&requests[0],NativeAction::Remote(host::RemoteRequest::Cancel{attempt_id})if attempt_id.to_string()==id)
    );
}
#[test]
fn auth_keys_are_validated_and_bound_as_redacted_native_credentials() {
    let (service, gateway) = fixture(Outcome::State(state()));
    let bad = "tskey-auth-not valid";
    assert!(
        !run(
            &service,
            api::Request::Login {
                auth_key: Some(bad.into())
            }
        )
        .err()
        .unwrap()
        .to_string()
        .contains(bad)
    );
    assert!(gateway.requests.lock().unwrap().is_empty());
    let key = "tskey-auth-owned_fixture_only";
    run(
        &service,
        api::Request::Login {
            auth_key: Some(key.into()),
        },
    )
    .unwrap();
    let requests = gateway.requests.lock().unwrap();
    assert!(matches!(
        &requests[0],
        NativeAction::Remote(host::RemoteRequest::Login { auth_key: Some(_) })
    ));
    assert!(!format!("{:?}", requests[0]).contains(key));
}
#[test]
fn a_wrong_response_variant_is_not_silent_success() {
    let (service, _) = fixture(Outcome::WrongResponse);
    assert!(run(&service, api::Request::RemoteState).is_err());
}
