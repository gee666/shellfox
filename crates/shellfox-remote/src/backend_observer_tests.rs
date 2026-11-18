use super::*;
use crate::{Approval, ViewPermission};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
const NOW: u64 = 1700000001123;
#[path = "backend_observer_replacement_tests.rs"]
mod replacement;
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
    serde_json::from_str::<Vec<Vector>>(include_str!(
        "../tests/fixtures/observer-auth-vectors.json"
    ))
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
            let OwnerCommand::ObserverV2(request) = command else {
                return Err(invalid());
            };
            if matches!(request.operation, ObserverOperation::Poll)
                && self.slow_poll.load(Ordering::SeqCst)
            {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            request.dispatch(|| match &request.operation {
                ObserverOperation::Pair => {
                    serde_json::to_value(request.connection.selection()).map_err(|_| invalid())
                }
                ObserverOperation::Disconnect => Ok(json!({"disconnected":true})),
                ObserverOperation::Poll => Ok(json!({"events":[]})),
                ObserverOperation::Execute(wire::Command::GetSnapshot(_)) => {
                    let fixture: Value = serde_json::from_str(include_str!(
                        "../../shellfox-protocol/tests/fixtures/observer_v2/snapshot.json"
                    ))
                    .unwrap();
                    Ok(fixture["value"].clone())
                }
                ObserverOperation::Execute(wire::Command::RenameSession(input)) => {
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
                ObserverOperation::Execute(_) => Err(invalid()),
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
    .prepare_observer_v2();
    backend.clock = || NOW;
    let observer = backend.observer_v2.as_mut().unwrap();
    observer.owner_id = "10000000-0000-4000-8000-000000000001".parse().unwrap();
    observer.mint_connection = || "40000000-0000-4000-8000-000000000001".parse().unwrap();
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
        .handle_observer_v2_signed(
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
async fn default_off_and_authenticated_discovery_have_no_approval_or_owner_side_effect() {
    let (mut backend, control, owner) = backend(None);
    backend.observer_v2 = None;
    let v = vector("discover");
    let response = backend
        .handle_observer_v2_signed(
            Endpoint::DiscoverV2,
            v.body.as_bytes(),
            SignedHeaders {
                peer: &v.peer,
                timestamp: &v.timestamp,
                signature: &v.signature,
            },
        )
        .await;
    assert_eq!(response.status, 404);
    assert!(response.body.is_empty());
    let backend = backend.prepare_observer_v2();
    let discovery = call(&backend, "discover").await;
    assert_eq!(discovery["value"]["profiles"], json!(["observer-v2"]));
    assert!(control.state().unwrap().pending.is_empty());
    assert_eq!(owner.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn signed_pair_complete_binding_profile_and_existing_permission_are_independent_checks() {
    let (backend, control, owner) = backend(Some(ViewPermission::ReadOnly));
    assert_eq!(call(&backend, "pair").await["ok"], true);
    let paired_calls = owner.calls.load(Ordering::SeqCst);
    assert_eq!(call(&backend, "pair-repeat").await["ok"], true);
    assert_eq!(
        owner.calls.load(Ordering::SeqCst),
        paired_calls,
        "same attempt is idempotent"
    );
    let snapshot = call(&backend, "snapshot").await;
    assert_eq!(snapshot["ok"], true);
    assert_eq!(snapshot["value"]["coverage"]["entryCount"], 3);
    let calls = owner.calls.load(Ordering::SeqCst);
    for name in ["wrong-client", "wrong-peer", "wrong-owner"] {
        assert_eq!(call(&backend, name).await["error"]["code"], "AUTH_FAILED");
    }
    assert_eq!(owner.calls.load(Ordering::SeqCst), calls);
    let denied = call(&backend, "write").await;
    assert_eq!(denied["error"]["code"], "UNSUPPORTED");
    assert_eq!(denied["error"]["detail"]["kind"], "profileIncompatible");
    assert_eq!(call(&backend, "rename").await["ok"], true);
    control.set_enabled(false).unwrap();
    assert_eq!(
        call(&backend, "snapshot-repeat").await["error"]["code"],
        "ACCESS_DENIED"
    );
}
#[tokio::test]
async fn attempt_withdrawal_does_not_deny_a_v1_client_with_the_same_wire_client_uuid() {
    let (backend, control, owner) = backend(Some(ViewPermission::Write));
    let peer = PeerIdentity {
        id: "n-peer".into(),
        hostname: "offline".into(),
    };
    let v1 = control
        .authorize(
            &peer,
            "20000000-0000-4000-8000-000000000001".parse().unwrap(),
            NOW,
        )
        .unwrap();
    assert_eq!(call(&backend, "pair").await["ok"], true);
    assert_eq!(call(&backend, "withdraw").await["ok"], true);
    v1.with_authorized(None, || Ok(())).unwrap();
    assert_eq!(
        call(&backend, "snapshot").await["error"]["code"],
        "AUTH_FAILED"
    );
    assert!(owner.calls.load(Ordering::SeqCst) >= 2);
    assert_eq!(
        call(&backend, "pair-repeat").await["error"]["code"],
        "AUTH_FAILED"
    );
}
#[tokio::test]
async fn cancellation_releases_the_one_poll_slot_without_any_synthetic_ack() {
    let (backend, _, owner) = backend(Some(ViewPermission::ReadOnly));
    assert_eq!(call(&backend, "pair").await["ok"], true);
    owner.slow_poll.store(true, Ordering::SeqCst);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), call(&backend, "events"))
            .await
            .is_err()
    );
    assert!(backend
        .observer_v2
        .as_ref()
        .unwrap()
        .state
        .lock()
        .unwrap()
        .views
        .values()
        .all(|view| !view.polling));
    assert!(
        tokio::time::timeout(Duration::from_millis(20), call(&backend, "events-repeat"))
            .await
            .is_err()
    );
}
#[tokio::test]
async fn hmac_binds_original_path_body_and_peer_before_json_or_approval() {
    let (backend, control, owner) = backend(None);
    let v = vector("pair");
    let mut changed = v.body.into_bytes();
    changed.push(b' ');
    let reply = backend
        .handle_observer_v2_signed(
            Endpoint::PairV2,
            &changed,
            SignedHeaders {
                peer: &v.peer,
                timestamp: &v.timestamp,
                signature: &v.signature,
            },
        )
        .await;
    let reply: Value = serde_json::from_slice(&reply.body).unwrap();
    assert_eq!(reply["error"]["code"], "AUTH_FAILED");
    assert_eq!(
        call(&backend, "invalid-json").await["error"]["code"],
        "VALIDATION"
    );
    assert!(control.state().unwrap().pending.is_empty());
    assert_eq!(owner.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        call(&backend, "pair").await["error"]["code"],
        "APPROVAL_REQUIRED"
    );
}
