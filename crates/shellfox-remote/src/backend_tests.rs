use super::*;
use crate::{Approval, ViewPermission};
use std::sync::atomic::{AtomicUsize, Ordering};
#[derive(Deserialize)]
struct Vector {
    name: String,
    path: String,
    body: String,
    peer: String,
    timestamp: String,
    signature: String,
}
pub(super) fn request(name: &str) -> http::Request {
    let vector =
        serde_json::from_str::<Vec<Vector>>(include_str!("../tests/fixtures/auth-vectors.json"))
            .unwrap()
            .into_iter()
            .find(|v| v.name == name)
            .unwrap();
    http::Request {
        path: vector.path,
        body: vector.body.into_bytes(),
        peer: vector.peer,
        timestamp: vector.timestamp,
        signature: vector.signature,
    }
}
#[derive(Default)]
struct Fake {
    calls: AtomicUsize,
}
impl RemoteHost for Fake {
    fn request(&self, command: OwnerCommand) -> crate::channel::OwnerFuture<'_> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let actor = command.actor().clone();
            if matches!(command, OwnerCommand::Disconnect { .. }) {
                return Ok(json!({"disconnected":true}));
            }
            actor.with_authorized(command.operation(), || {
                Ok(match command {
                    OwnerCommand::Poll { .. } => json!({"events":[]}),
                    _ => json!({}),
                })
            })
        })
    }
}
fn approve(control: &ApprovalControl, permission: ViewPermission) {
    control
        .install(Approval {
            peer_id: "n-peer".into(),
            hostname: "my laptop".into(),
            approved_at_millis: 1700000000123,
            permission,
            file_roots: vec![],
        })
        .unwrap();
}
fn backend(control: ApprovalControl, fake: Arc<Fake>) -> Backend {
    let mut backend = Backend::new(fake, control, Secret::new("b".repeat(32)).unwrap());
    backend.clock = || NOW;
    backend
}
const NOW: u64 = 1700000000123;
#[tokio::test]
async fn backwards_clock_refuses_admission_before_json_until_corrected() {
    let mut backend = backend(ApprovalControl::default(), Arc::new(Fake::default()));
    backend.handle_at(request("discover"), NOW).await.unwrap();
    backend.clock = || NOW - 1;
    assert_eq!(
        backend
            .handle_at(request("invalid-json"), NOW - 1)
            .await
            .unwrap_err()
            .code,
        ErrorCode::AuthFailed
    );
    backend.clock = || NOW;
    assert_eq!(
        backend
            .handle_at(request("invalid-json"), NOW)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
}
#[test]
fn authenticated_peer_rate_is_bounded_before_dispatch() {
    let backend = backend(ApprovalControl::default(), Arc::new(Fake::default()));
    let peer = PeerIdentity {
        id: "n-peer".into(),
        hostname: "offline".into(),
    };
    for i in 0..240 {
        backend.limit(&peer, &format!("{i:064x}"), NOW).unwrap();
    }
    assert_eq!(
        backend
            .limit(&peer, &format!("{:064x}", 240), NOW)
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
}
#[tokio::test]
async fn hmac_before_json_and_approval_before_owner() {
    let control = ApprovalControl::default();
    let fake = Arc::new(Fake::default());
    let backend = backend(control.clone(), fake.clone());
    let mut invalid = request("invalid-json");
    invalid.signature = "0".repeat(64);
    assert_eq!(
        backend.handle_at(invalid, NOW).await.unwrap_err().code,
        ErrorCode::AuthFailed
    );
    assert_eq!(
        backend
            .handle_at(request("invalid-json"), NOW)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    assert_eq!(
        backend
            .handle_at(request("pair"), NOW)
            .await
            .unwrap_err()
            .code,
        ErrorCode::ApprovalRequired
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    assert_eq!(control.state().unwrap().pending.len(), 1);
    assert_eq!(
        backend.handle_at(request("discover"), NOW).await.unwrap()["protocolVersion"],
        1
    );
    assert_eq!(
        backend
            .handle_at(request("discover"), NOW)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    assert_eq!(
        backend
            .handle_at(request("snapshot"), NOW + 30_001)
            .await
            .unwrap_err()
            .code,
        ErrorCode::AuthFailed
    );
}
#[tokio::test]
async fn known_legacy_viewonly_migrates_to_control_but_revoke_blocks_old_actor_and_disconnect_cleans(
) {
    let control = ApprovalControl::default();
    approve(&control, ViewPermission::ReadOnly);
    let fake = Arc::new(Fake::default());
    let backend = backend(control.clone(), fake.clone());
    assert_eq!(
        backend.handle_at(request("pair"), NOW).await.unwrap()["paired"],
        true
    );
    assert!(backend.handle_at(request("write"), NOW).await.is_ok());
    assert_eq!(
        control.state().unwrap().approvals[0].permission,
        ViewPermission::Write
    );
    assert_eq!(
        backend
            .handle_at(request("privileged"), NOW)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    let peer = PeerIdentity {
        id: "n-peer".into(),
        hostname: "my laptop".into(),
    };
    let actor = control
        .authorize(
            &peer,
            "11111111-1111-4111-8111-111111111111".parse().unwrap(),
            NOW,
        )
        .unwrap();
    control.reject_inner("n-peer", true).unwrap();
    assert_eq!(
        actor.with_authorized(None, || Ok(())).unwrap_err().code,
        ErrorCode::AccessDenied
    );
    assert_eq!(
        backend
            .handle_at(request("snapshot"), NOW)
            .await
            .unwrap_err()
            .code,
        ErrorCode::AccessDenied
    );
    assert_eq!(
        backend.handle_at(request("disconnect"), NOW).await.unwrap()["disconnected"],
        true
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 3);
}
#[tokio::test]
async fn raw_signed_path_body_and_client_identity_are_bound() {
    for change in ["path", "body", "peer"] {
        let fake = Arc::new(Fake::default());
        let backend = backend(ApprovalControl::default(), fake);
        let mut signed = request("pair");
        match change {
            "path" => signed.path = "/v1/request".into(),
            "body" => signed.body.push(b' '),
            _ => signed.peer.push('a'),
        }
        assert_eq!(
            backend.handle_at(signed, NOW).await.unwrap_err().code,
            ErrorCode::AuthFailed
        );
    }
    assert!(serde_json::from_value::<Identity>(
        json!({"clientId":"11111111111141118111111111111111"})
    )
    .is_err());
    assert!(serde_json::from_value::<Identity>(
        json!({"clientId":"11111111-1111-4111-8111-111111111111","permission":"write"})
    )
    .is_err());
}
