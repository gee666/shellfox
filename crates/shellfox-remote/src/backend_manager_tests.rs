use super::*;
#[path = "backend_manager_input_tests.rs"]
mod input;
use crate::{Approval, ViewPermission};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
const NOW: u64 = 1700000001123;
#[derive(Deserialize)]
struct Vector {
    name: String,
    path: String,
    body: String,
    peer: String,
    timestamp: String,
    signature: String,
}
fn vector(name: &str) -> Vector {
    serde_json::from_str::<Vec<Vector>>(include_str!("../tests/fixtures/manager-auth-vectors.json"))
        .unwrap()
        .into_iter()
        .find(|v| v.name == name)
        .unwrap()
}
#[derive(Default)]
struct FakeOwner {
    calls: AtomicUsize,
    slow_poll: AtomicBool,
}
impl RemoteHost for FakeOwner {
    fn request(&self, command: OwnerCommand) -> crate::channel::OwnerFuture<'_> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let OwnerCommand::ManagerV2(request) = command else {
                return Err(invalid());
            };
            if matches!(request.operation, ManagerOperation::Poll)
                && self.slow_poll.load(Ordering::SeqCst)
            {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            request.dispatch(|| match &request.operation {
                ManagerOperation::Pair => {
                    serde_json::to_value(request.connection.selection()).map_err(|_| invalid())
                }
                ManagerOperation::Disconnect => Ok(json!({"disconnected":true})),
                ManagerOperation::Poll => Ok(json!({"events":[]})),
                ManagerOperation::Execute(wire::Command::GetSnapshot(_)) => {
                    let fixture: Value = serde_json::from_str(include_str!(
                        "../../shellfox-protocol/tests/fixtures/observer_v2/snapshot.json"
                    ))
                    .unwrap();
                    Ok(fixture["value"].clone())
                }
                ManagerOperation::Execute(wire::Command::RenameSession(input)) => {
                    let fixture: Value = serde_json::from_str(include_str!(
                        "../../shellfox-protocol/tests/fixtures/observer_v2/snapshot.json"
                    ))
                    .unwrap();
                    let mut entry = fixture["value"]["entries"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|entry| entry["value"]["id"] == input.0.session_id.to_string())
                        .unwrap()
                        .clone();
                    entry["value"]["title"] = Value::String(input.0.title.clone());
                    Ok(entry)
                }
                ManagerOperation::Execute(wire::Command::WriteTerminal(input)) => {
                    if let wire::Input::Key { event } = &input.input {
                        assert_eq!(event.physical, wire::PhysicalKey::Space);
                    }
                    Err(request.failure.report(wire::Error {
                        code: ErrorCode::Validation,
                        message: "No write: owned encoder budget exceeded.".into(),
                        retryable: false,
                        detail: wire::ErrorDetail::ResourceLimit {
                            limit: wire::Limit::EncodedInputBytes,
                        },
                    }))
                }
                ManagerOperation::Execute(_) => Err(invalid()),
            })
        })
    }
}
fn backend(permission: Option<ViewPermission>) -> (Backend, ApprovalControl, Arc<FakeOwner>) {
    let control = ApprovalControl::default();
    if let Some(permission) = permission {
        control
            .install(Approval {
                peer_id: "n-peer".into(),
                hostname: "offline".into(),
                approved_at_millis: 1,
                permission,
                file_roots: Vec::new(),
            })
            .unwrap();
    }
    let owner = Arc::new(FakeOwner::default());
    let mut backend = Backend::new(
        owner.clone(),
        control.clone(),
        Secret::new("b".repeat(32)).unwrap(),
    )
    .prepare_manager_v2();
    backend.clock = || NOW;
    let manager = backend.manager_v2.as_mut().unwrap();
    manager.owner_id = "10000000-0000-4000-8000-000000000001".parse().unwrap();
    manager.mint_connection = || "40000000-0000-4000-8000-000000000001".parse().unwrap();
    (backend, control, owner)
}
async fn call(backend: &Backend, name: &str) -> Value {
    let vector = vector(name);
    let endpoint = match vector.path.as_str() {
        "/v2/discover" => Endpoint::DiscoverV2,
        "/v2/pair" => Endpoint::PairV2,
        "/v2/request" => Endpoint::RequestV2,
        "/v2/events" => Endpoint::EventsV2,
        "/v2/disconnect" => Endpoint::DisconnectV2,
        _ => panic!("fixed fixture route"),
    };
    let reply = backend
        .handle_manager_v2_signed(
            endpoint,
            vector.body.as_bytes(),
            SignedHeaders {
                peer: &vector.peer,
                timestamp: &vector.timestamp,
                signature: &vector.signature,
            },
        )
        .await;
    assert_eq!(reply.status, 200);
    serde_json::from_slice(&reply.body).unwrap()
}
#[tokio::test]
async fn default_off_discovery_and_fixed_approved_device_policy() {
    let (mut backend, control, owner) = backend(None);
    backend.manager_v2 = None;
    let v = vector("discover");
    let reply = backend
        .handle_manager_v2_signed(
            Endpoint::DiscoverV2,
            v.body.as_bytes(),
            SignedHeaders {
                peer: &v.peer,
                timestamp: &v.timestamp,
                signature: &v.signature,
            },
        )
        .await;
    assert_eq!(reply.status, 404);
    assert!(reply.body.is_empty());
    let backend = backend.prepare_manager_v2();
    assert_eq!(
        call(&backend, "discover").await["value"]["profiles"],
        json!(["manager-v2"])
    );
    assert!(control.state().unwrap().pending.is_empty());
    assert_eq!(owner.calls.load(Ordering::SeqCst), 0);
    let (backend, _, _) = super::tests::backend(Some(ViewPermission::ReadOnly));
    let paired = call(&backend, "pair").await;
    assert_eq!(paired["ok"], true);
    assert_eq!(paired["value"]["devicePermission"], "write");
    assert_eq!(paired["value"]["devicePolicy"], "control-full-account-v1");
    assert_eq!(
        call(&backend, "snapshot").await["value"]["coverage"]["entryCount"],
        3
    );
}
#[tokio::test]
async fn exact_binding_current_approval_and_typed_encoded_limit_are_independent() {
    let (backend, control, owner) = backend(Some(ViewPermission::Write));
    assert_eq!(call(&backend, "pair").await["ok"], true);
    let calls = owner.calls.load(Ordering::SeqCst);
    for name in ["wrong-client", "wrong-peer", "wrong-owner"] {
        assert_eq!(call(&backend, name).await["error"]["code"], "AUTH_FAILED");
    }
    assert_eq!(owner.calls.load(Ordering::SeqCst), calls);
    let rejected = call(&backend, "write").await;
    assert_eq!(rejected["error"]["detail"]["kind"], "resourceLimit");
    assert_eq!(rejected["error"]["detail"]["limit"], "encodedInputBytes");
    assert_eq!(rejected["error"]["retryable"], false);
    let key = call(&backend, "write-key").await;
    assert_eq!(key["error"]["detail"]["limit"], "encodedInputBytes");
    control.set_enabled(false).unwrap();
    assert_eq!(
        call(&backend, "snapshot-repeat").await["error"]["code"],
        "ACCESS_DENIED"
    );
}
#[tokio::test]
async fn original_mac_bytes_path_peer_and_no_signature_replay() {
    let (backend, _, owner) = backend(Some(ViewPermission::Write));
    let v = vector("pair");
    for (endpoint, body, peer) in [
        (Endpoint::RequestV2, v.body.clone(), v.peer.clone()),
        (Endpoint::PairV2, format!("{} ", v.body), v.peer.clone()),
        (Endpoint::PairV2, v.body.clone(), vector("wrong-peer").peer),
    ] {
        let response = backend
            .handle_manager_v2_signed(
                endpoint,
                body.as_bytes(),
                SignedHeaders {
                    peer: &peer,
                    timestamp: &v.timestamp,
                    signature: &v.signature,
                },
            )
            .await;
        let value: Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(value["error"]["code"], "AUTH_FAILED");
    }
    assert_eq!(owner.calls.load(Ordering::SeqCst), 0);
    assert_eq!(call(&backend, "pair").await["ok"], true);
    assert_eq!(call(&backend, "pair").await["error"]["code"], "VALIDATION");
}
#[tokio::test]
async fn canceled_poll_releases_the_same_connection_slot() {
    let (backend, _, owner) = backend(Some(ViewPermission::Write));
    assert_eq!(call(&backend, "pair").await["ok"], true);
    owner.slow_poll.store(true, Ordering::SeqCst);
    assert!(
        tokio::time::timeout(Duration::from_millis(5), call(&backend, "events"))
            .await
            .is_err()
    );
    {
        let state = backend.manager_v2.as_ref().unwrap().state.lock().unwrap();
        assert!(state.views.values().all(|view| !view.polling));
    }
    assert_eq!(call(&backend, "rename").await["ok"], true);
}
