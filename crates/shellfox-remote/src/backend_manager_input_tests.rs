//! Injected owner-boundary proofs; these are not Native/PTY acceptance tests.
use super::*;
use crate::{Approval, ViewPermission};
use std::sync::atomic::{AtomicUsize, Ordering};
#[derive(Clone, Copy)]
enum Proof {
    Completed,
    Ignored,
    Malformed,
    Mismatch,
    WrongKind,
    Unknown,
    RevokeBefore,
    MetadataRevoked,
    PollRevoked,
}
struct ProofOwner {
    proof: Proof,
    control: ApprovalControl,
    executed: AtomicUsize,
}
impl RemoteHost for ProofOwner {
    fn request(&self, command: OwnerCommand) -> crate::channel::OwnerFuture<'_> {
        Box::pin(async move {
            let OwnerCommand::ManagerV2(request) = command else {
                return Err(invalid());
            };
            let input = matches!(
                request.operation,
                ManagerOperation::Execute(wire::Command::WriteTerminal(_))
            );
            let noninput_revoke = matches!(
                (&self.proof, &request.operation),
                (
                    Proof::MetadataRevoked,
                    ManagerOperation::Execute(wire::Command::GetSnapshot(_))
                ) | (Proof::PollRevoked, ManagerOperation::Poll)
            );
            if input && matches!(self.proof, Proof::RevokeBefore) {
                self.control.set_enabled(false)?;
            }
            let result = request.dispatch(|| match &request.operation {
                ManagerOperation::Pair => {
                    serde_json::to_value(request.connection.selection()).map_err(|_| invalid())
                }
                ManagerOperation::Execute(wire::Command::GetSnapshot(_)) => {
                    let fixture: Value = serde_json::from_str(include_str!(
                        "../../shellfox-protocol/tests/fixtures/observer_v2/snapshot.json"
                    ))
                    .unwrap();
                    Ok(fixture["value"].clone())
                }
                ManagerOperation::Poll => Ok(json!({"events":[]})),
                ManagerOperation::Execute(wire::Command::WriteTerminal(input)) => {
                    self.executed.fetch_add(1, Ordering::SeqCst);
                    if matches!(self.proof, Proof::Unknown) {
                        return Err(crate::error(
                            ErrorCode::NativeUnavailable,
                            "Injected uncertain owner/write result.",
                            false,
                        ));
                    }
                    if matches!(self.proof, Proof::Malformed) {
                        return Ok(json!({"configured":true}));
                    }
                    let receipt = wire::AcceptedInput {
                        input_id: if matches!(self.proof, Proof::Mismatch) {
                            wire::Id::new()
                        } else {
                            input.input_id
                        },
                        kind: if matches!(self.proof, Proof::WrongKind) {
                            wire::InputKind::Key
                        } else {
                            input.input.kind()
                        },
                        accepted_bytes: if matches!(self.proof, Proof::Ignored) {
                            0
                        } else {
                            5
                        },
                        disposition: if matches!(self.proof, Proof::Ignored) {
                            wire::Disposition::Ignored
                        } else {
                            wire::Disposition::Accepted
                        },
                    };
                    serde_json::to_value(receipt).map_err(|_| invalid())
                }
                _ => Err(invalid()),
            });
            if (input && matches!(self.proof, Proof::Completed | Proof::Ignored)) || noninput_revoke
            {
                // Simulate revocation winning AFTER a committed result, outside its guard.
                self.control.set_enabled(false)?;
                request.connection.invalidate();
            }
            result
        })
    }
}
fn setup(proof: Proof) -> (Backend, Arc<ProofOwner>) {
    let control = ApprovalControl::default();
    control
        .install(Approval {
            peer_id: "n-peer".into(),
            hostname: "offline".into(),
            approved_at_millis: 1,
            permission: ViewPermission::Write,
            file_roots: Vec::new(),
        })
        .unwrap();
    let owner = Arc::new(ProofOwner {
        proof,
        control: control.clone(),
        executed: AtomicUsize::new(0),
    });
    let mut backend = Backend::new(owner.clone(), control, Secret::new("b".repeat(32)).unwrap())
        .prepare_manager_v2();
    backend.clock = || NOW;
    let manager = backend.manager_v2.as_mut().unwrap();
    manager.owner_id = "10000000-0000-4000-8000-000000000001".parse().unwrap();
    manager.mint_connection = || "40000000-0000-4000-8000-000000000001".parse().unwrap();
    (backend, owner)
}
#[tokio::test]
async fn revoke_before_owner_admit_has_no_input_side_effect() {
    let (backend, owner) = setup(Proof::RevokeBefore);
    assert_eq!(call(&backend, "pair").await["ok"], true);
    let denied = call(&backend, "write").await;
    assert_eq!(denied["error"]["code"], "ACCESS_DENIED");
    assert_eq!(denied["error"]["detail"]["kind"], "application");
    assert_eq!(owner.executed.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn historical_completed_receipt_survives_postcommit_revoke_without_authority_resurrection() {
    let (backend, owner) = setup(Proof::Completed);
    assert_eq!(call(&backend, "pair").await["ok"], true);
    let receipt = call(&backend, "write").await;
    assert_eq!(receipt["ok"], true);
    assert_eq!(receipt["value"]["acceptedBytes"], 5);
    assert_eq!(receipt["value"]["kind"], "paste");
    assert_eq!(receipt["value"]["disposition"], "accepted");
    let original: Value = serde_json::from_str(&vector("write").body).unwrap();
    assert_eq!(receipt["binding"], original["binding"]);
    assert_eq!(receipt["value"]["inputId"], original["payload"]["inputId"]);
    assert!(receipt["value"].get("control").is_none());
    assert_eq!(call(&backend, "write-key").await["ok"], false);
    assert_eq!(owner.executed.load(Ordering::SeqCst), 1);
    assert!(backend
        .manager_v2
        .as_ref()
        .unwrap()
        .state
        .lock()
        .unwrap()
        .views
        .values()
        .all(|view| view
            .connection
            .check(
                &AuthenticatedPeerId::new("n-peer").unwrap(),
                &view.connection.binding()
            )
            .is_err()));
}
#[tokio::test]
async fn historical_actual_ignored_zero_is_not_synthesized_from_error() {
    let (backend, owner) = setup(Proof::Ignored);
    assert_eq!(call(&backend, "pair").await["ok"], true);
    let receipt = call(&backend, "write-mouse").await;
    assert_eq!(receipt["ok"], true);
    assert_eq!(receipt["value"]["acceptedBytes"], 0);
    assert_eq!(receipt["value"]["kind"], "mouse");
    assert_eq!(receipt["value"]["disposition"], "ignored");
    assert_eq!(owner.executed.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn malformed_mismatched_and_unknown_write_never_become_fake_zero_receipts() {
    for proof in [
        Proof::Malformed,
        Proof::Mismatch,
        Proof::WrongKind,
        Proof::Unknown,
    ] {
        let (backend, owner) = setup(proof);
        assert_eq!(call(&backend, "pair").await["ok"], true);
        let response = call(&backend, "write").await;
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["detail"]["kind"], "inputOutcomeUnknown");
        assert_eq!(response["error"]["detail"]["retryInput"], false);
        assert_eq!(response["error"]["retryable"], false);
        assert!(response.get("value").is_none());
        assert_eq!(owner.executed.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn binary_receipt_must_report_exact_encoded_count_not_a_partial_or_synthesized_count() {
    let (backend, _) = setup(Proof::Completed);
    assert_eq!(call(&backend, "pair").await["ok"], true);
    let response = call(&backend, "write-bytes").await;
    assert_eq!(response["error"]["detail"]["kind"], "inputOutcomeUnknown");
    assert_eq!(response["error"]["detail"]["retryInput"], false);
    assert!(response.get("value").is_none());
}
#[tokio::test]
async fn metadata_and_poll_results_are_not_historical_receipt_exemptions() {
    for (proof, name) in [
        (Proof::MetadataRevoked, "snapshot"),
        (Proof::PollRevoked, "events"),
    ] {
        let (backend, owner) = setup(proof);
        assert_eq!(call(&backend, "pair").await["ok"], true);
        let response = call(&backend, name).await;
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], "ACCESS_DENIED");
        assert!(response.get("value").is_none());
        assert_eq!(owner.executed.load(Ordering::SeqCst), 0);
    }
}
