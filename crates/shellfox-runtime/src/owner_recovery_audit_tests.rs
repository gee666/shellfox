use super::*;
use std::sync::mpsc;
fn claim(owner: &mut Owner) -> Lease {
    match owner
        .command(
            RequestId(1),
            Command::Claim {
                controller: 1,
                takeover: false,
            },
        )
        .unwrap()
    {
        Response::Claimed(lease) => lease,
        _ => panic!(),
    }
}
fn observe(owner: &mut Owner) -> ObserverLease {
    match owner
        .command(
            RequestId(2),
            Command::Observe {
                viewer: 20,
                after: None,
            },
        )
        .unwrap()
    {
        Response::Observed(observer) => observer,
        _ => panic!(),
    }
}
fn recovery(owner: &mut Owner, command: Command) -> StampedRecovery {
    match owner.command(RequestId(3), command).unwrap() {
        Response::Recovery(snapshot) => snapshot,
        _ => panic!(),
    }
}
fn control_recover(lease: Lease) -> Command {
    Command::RecoverStamped {
        lease,
        capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
    }
}
fn observer_recover(observer: ObserverLease) -> Command {
    Command::ObserverRecoverStamped {
        observer,
        capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
    }
}
fn control_ack(lease: Lease, snapshot: &StampedRecovery) -> Command {
    Command::ControllerLayoutAck {
        lease,
        recovery_id: snapshot.recovery_id,
        last_delivery_sequence: snapshot.last_delivery_sequence,
        layout_revision: snapshot.layout_revision,
    }
}
fn observer_ack(observer: ObserverLease, snapshot: &StampedRecovery) -> Command {
    Command::ObserverLayoutAck {
        observer,
        recovery_id: snapshot.recovery_id,
        last_delivery_sequence: snapshot.last_delivery_sequence,
        layout_revision: snapshot.layout_revision,
    }
}
#[test]
fn ordinary_formatter_failure_keeps_both_prior_pending_tokens_and_credit_barriers() {
    let mut owner = test_support::owner();
    let lease = claim(&mut owner);
    let observer = observe(&mut owner);
    let controller = recovery(&mut owner, control_recover(lease));
    let observed = recovery(&mut owner, observer_recover(observer));
    owner.accept_source_bytes(b"\x1b]0;").unwrap();
    owner
        .accept_source_bytes(&vec![b'x'; shellfox_ghostty_vt::CONTINUATION_MAX_BYTES + 1])
        .unwrap();
    let before = (
        owner.delivery.last_sent(),
        owner.observers.sent_sequence(observer),
    );
    for command in [control_recover(lease), observer_recover(observer)] {
        assert_eq!(
            owner.command(RequestId(4), command).unwrap_err().code,
            ErrorCode::Unsupported
        );
    }
    assert_eq!(owner.delivery.lease(), Some(lease));
    owner.observers.check(observer).unwrap();
    assert_eq!(
        (
            owner.delivery.last_sent(),
            owner.observers.sent_sequence(observer)
        ),
        before
    );
    assert_eq!(
        owner
            .command(RequestId(5), Command::CheckControllerReady { lease })
            .unwrap_err()
            .code,
        ErrorCode::RecoveryRequired
    );
    // A failed replacement did not supersede either original exact token.
    owner
        .command(RequestId(6), control_ack(lease, &controller))
        .unwrap();
    owner
        .command(RequestId(7), observer_ack(observer, &observed))
        .unwrap();
}
#[test]
fn dropped_successful_controller_recovery_keeps_same_lease_and_explicit_fresh_recovery_supersedes_without_ack(
) {
    let mut owner = test_support::owner();
    let lease = claim(&mut owner);
    owner.accept_source_bytes(b"before").unwrap();
    // Inspect the real success only as a leaf-test oracle; the simulated caller loses it.
    let lost = recovery(&mut owner, control_recover(lease));
    let old_id = lost.recovery_id;
    let old_sequence = lost.last_delivery_sequence;
    let old_revision = lost.layout_revision;
    drop(lost);
    owner.accept_source_bytes(b" later").unwrap();
    assert_eq!(owner.delivery.lease(), Some(lease));
    assert_eq!(owner.delivery.last_sent(), old_sequence);
    assert_eq!(
        owner
            .command(RequestId(8), Command::CheckControllerReady { lease })
            .unwrap_err()
            .code,
        ErrorCode::RecoveryRequired
    );
    assert_eq!(
        owner
            .command(
                RequestId(9),
                Command::Ack {
                    lease,
                    sequence: old_sequence
                }
            )
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    let (send, receive) = mpsc::sync_channel(8);
    owner.emit_events(&send);
    assert!(receive.try_recv().is_err());
    let fresh = recovery(&mut owner, control_recover(lease));
    assert_ne!(fresh.recovery_id, old_id);
    assert_eq!(
        fresh.last_delivery_sequence,
        owner.delivery.current_sequence()
    );
    assert_eq!(owner.delivery.lease(), Some(lease));
    assert_eq!(
        owner
            .command(
                RequestId(10),
                Command::ControllerLayoutAck {
                    lease,
                    recovery_id: old_id,
                    last_delivery_sequence: old_sequence,
                    layout_revision: old_revision
                }
            )
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    assert!(owner.layouts.controller_ready(lease).is_err());
    owner
        .command(RequestId(11), control_ack(lease, &fresh))
        .unwrap();
    owner.accept_source_bytes(b" suffix").unwrap();
    owner.emit_events(&send);
    assert!(
        matches!(receive.try_recv().unwrap(),Event::Output{lease:actual,chunk}if actual==lease && chunk.sequence==fresh.last_delivery_sequence+1)
    );
}
#[test]
fn dropped_successful_observer_recovery_keeps_real_observer_and_only_exact_fresh_snapshot_releases_it(
) {
    let mut owner = test_support::owner();
    let lease = claim(&mut owner);
    let control = recovery(&mut owner, control_recover(lease));
    owner
        .command(RequestId(12), control_ack(lease, &control))
        .unwrap();
    let observer = observe(&mut owner);
    owner.accept_source_bytes(b"before").unwrap();
    let lost = recovery(&mut owner, observer_recover(observer));
    let id = lost.recovery_id;
    let sequence = lost.last_delivery_sequence;
    let revision = lost.layout_revision;
    drop(lost);
    owner.accept_source_bytes(b" later").unwrap();
    owner.observers.check(observer).unwrap();
    assert_eq!(owner.observers.sent_sequence(observer), Some(sequence));
    assert_eq!(
        owner
            .command(RequestId(13), Command::ObserverAck { observer, sequence })
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    assert!(
        owner.layouts.controller_ready(lease).is_ok(),
        "observer recovery cannot block the controller input gate"
    );
    let (send, receive) = mpsc::sync_channel(8);
    owner.emit_events(&send);
    assert!(!receive
        .try_iter()
        .any(|event| matches!(event, Event::ObserverOutput { .. })));
    let fresh = recovery(&mut owner, observer_recover(observer));
    assert_ne!(fresh.recovery_id, id);
    owner.observers.check(observer).unwrap();
    assert_eq!(
        owner
            .command(
                RequestId(14),
                Command::ObserverLayoutAck {
                    observer,
                    recovery_id: id,
                    last_delivery_sequence: sequence,
                    layout_revision: revision
                }
            )
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    owner
        .command(RequestId(15), observer_ack(observer, &fresh))
        .unwrap();
    owner.accept_source_bytes(b" suffix").unwrap();
    owner.emit_events(&send);
    assert!(receive.try_iter().any(|event|matches!(event,Event::ObserverOutput{observer:actual,chunk}if actual==observer && chunk.sequence==fresh.last_delivery_sequence+1)));
}
