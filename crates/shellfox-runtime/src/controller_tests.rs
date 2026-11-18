use super::*;
#[path = "controller_geometry_tests.rs"]
mod geometry;
#[path = "controller_journal_tests.rs"]
mod journal;
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
        other => panic!("{other:?}"),
    }
}
fn recover(owner: &mut Owner, lease: Lease) -> StampedRecovery {
    match owner
        .command(
            RequestId(2),
            Command::RecoverStamped {
                lease,
                capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
            },
        )
        .unwrap()
    {
        Response::Recovery(snapshot) => snapshot,
        other => panic!("{other:?}"),
    }
}
fn ack(
    owner: &mut Owner,
    lease: Lease,
    snapshot: &StampedRecovery,
) -> Result<Response, RuntimeError> {
    owner.command(
        RequestId(3),
        Command::ControllerLayoutAck {
            lease,
            recovery_id: snapshot.recovery_id,
            last_delivery_sequence: snapshot.last_delivery_sequence,
            layout_revision: snapshot.layout_revision,
        },
    )
}
fn ready(owner: &mut Owner, lease: Lease) -> Result<Response, RuntimeError> {
    owner.command(RequestId(4), Command::CheckControllerReady { lease })
}
fn stamp(snapshot: &StampedRecovery) -> ControllerGeometryStamp {
    ControllerGeometryStamp {
        recovery_id: snapshot.recovery_id,
        layout_revision: snapshot.layout_revision,
        cols: snapshot.cols,
        rows: snapshot.rows,
    }
}
#[test]
fn controller_snapshot_replacement_has_real_atomic_geometry_and_no_scalar_ack_or_legacy_downgrade()
{
    let mut owner = test_support::owner();
    let lease = claim(&mut owner);
    owner.accept_source_bytes(b"actual parsed bytes").unwrap();
    let first = recover(&mut owner, lease);
    let latest = recover(&mut owner, lease);
    assert_eq!(
        (
            latest.cols,
            latest.rows,
            latest.layout_revision,
            latest.last_delivery_sequence
        ),
        (80, 24, 1, 1)
    );
    assert_ne!(first.recovery_id, latest.recovery_id);
    assert_eq!(
        ready(&mut owner, lease).unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    assert!(ack(&mut owner, lease, &first).is_err());
    for command in [
        Command::Ack {
            lease,
            sequence: latest.last_delivery_sequence,
        },
        Command::RecoverAnsi { lease },
        Command::Recover {
            lease,
            capability: AnsiRecoveryCapability::VisibleScreen,
        },
        Command::Replay {
            lease,
            after: latest.last_delivery_sequence,
        },
    ] {
        assert_eq!(
            owner.command(RequestId(5), command).unwrap_err().code,
            ErrorCode::Validation
        );
    }
    ack(&mut owner, lease, &latest).unwrap();
    assert!(ready(&mut owner, lease).is_ok());
    assert!(ack(&mut owner, lease, &latest).is_err());
    assert_eq!(owner.delivery.current_sequence(), 1);
}
#[test]
fn pending_epoch_gates_all_interactive_side_effects_before_encode_write_resize_or_colors() {
    let mut owner = test_support::owner();
    let lease = claim(&mut owner);
    let snapshot = recover(&mut owner, lease);
    let commands = [
        Command::InteractiveBytes {
            lease,
            bytes: vec![0, 128, 255],
        },
        Command::RawInput {
            lease,
            bytes: b"legacy".to_vec(),
        },
        Command::Paste {
            lease,
            text: "paste".into(),
        },
        Command::Key {
            lease,
            event: shellfox_ghostty_vt::KeyEvent::default(),
        },
        Command::Resize {
            lease,
            cols: 100,
            rows: 30,
        },
        Command::MouseStamped {
            lease,
            event: mouse(),
            geometry: shellfox_ghostty_vt::MouseGeometry {
                cell_width: 8,
                cell_height: 16,
            },
            stamp: stamp(&snapshot),
        },
    ];
    for command in commands {
        assert_eq!(
            owner.command(RequestId(6), command).unwrap_err().code,
            ErrorCode::RecoveryRequired
        );
    }
    assert_eq!(
        (
            owner.terminal.render().unwrap().cols,
            owner.terminal.render().unwrap().rows
        ),
        (80, 24)
    );
    ack(&mut owner, lease, &snapshot).unwrap();
    assert!(ready(&mut owner, lease).is_ok());
    owner.canonical_resize(100, 30).unwrap();
    assert_eq!(
        ready(&mut owner, lease).unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    let latest = recover(&mut owner, lease);
    ack(&mut owner, lease, &latest).unwrap();
    assert!(ready(&mut owner, lease).is_ok());
}
#[test]
fn controller_layout_blocks_post_boundary_bytes_and_full_event_queue_commits_no_credit() {
    let mut owner = test_support::owner();
    let lease = claim(&mut owner);
    let first = recover(&mut owner, lease);
    ack(&mut owner, lease, &first).unwrap();
    owner.accept_source_bytes(b"old-grid").unwrap();
    owner.canonical_resize(100, 30).unwrap();
    owner.accept_source_bytes(b"new-grid").unwrap();
    let (send, receive) = mpsc::sync_channel(1);
    send.send(Event::Started(Ok(()))).unwrap();
    owner.emit_events(&send);
    assert_eq!(owner.delivery.last_sent(), 0);
    receive.try_recv().unwrap();
    owner.emit_events(&send);
    assert!(matches!(
        receive.try_recv().unwrap(),
        Event::Output {
            chunk: Chunk { sequence: 1, .. },
            ..
        }
    ));
    owner.emit_events(&send);
    assert!(
        matches!(receive.try_recv().unwrap(), Event::ControllerLayout { lease: actual, layout: LayoutStamp { last_delivery_sequence: 1, layout_revision: 2, cols:100, rows:30 } } if actual == lease)
    );
    for _ in 0..3 {
        owner.emit_events(&send);
        assert!(receive.try_recv().is_err());
        assert_eq!(owner.delivery.last_sent(), 1);
    }
    let latest = recover(&mut owner, lease);
    assert_eq!(latest.last_delivery_sequence, 2);
    ack(&mut owner, lease, &latest).unwrap();
    owner.accept_source_bytes(b"after-consumption").unwrap();
    owner.emit_events(&send);
    assert!(matches!(
        receive.try_recv().unwrap(),
        Event::Output {
            chunk: Chunk { sequence: 3, .. },
            ..
        }
    ));
}
#[test]
fn foreign_runtime_observer_and_old_epoch_recovery_ids_cannot_accept_controller_fence() {
    let mut owner = test_support::owner();
    let lease = claim(&mut owner);
    let snapshot = recover(&mut owner, lease);
    let mut foreign = test_support::owner();
    let other_lease = claim(&mut foreign);
    let other = recover(&mut foreign, other_lease);
    assert_ne!(snapshot.recovery_id, other.recovery_id);
    assert!(ack(&mut owner, lease, &other).is_err());
    let observer = match owner
        .command(
            RequestId(10),
            Command::Observe {
                viewer: 1,
                after: None,
            },
        )
        .unwrap()
    {
        Response::Observed(observer) => observer,
        _ => panic!(),
    };
    let observed = match owner
        .command(
            RequestId(11),
            Command::ObserverRecoverStamped {
                observer,
                capability: AnsiRecoveryCapability::VisibleScreen,
            },
        )
        .unwrap()
    {
        Response::Recovery(snapshot) => snapshot,
        _ => panic!(),
    };
    assert!(ack(&mut owner, lease, &observed).is_err());
    owner
        .command(RequestId(12), Command::ReleaseControl { lease })
        .unwrap();
    let new_lease = claim(&mut owner);
    let new = recover(&mut owner, new_lease);
    assert!(ack(&mut owner, new_lease, &snapshot).is_err());
    assert!(ack(&mut owner, lease, &new).is_err());
    ack(&mut owner, new_lease, &new).unwrap();
    assert!(owner.observers.check(observer).is_ok());
}
#[test]
fn release_and_takeover_revoke_in_order_leave_owned_source_active_and_require_real_new_epoch() {
    let mut owner = test_support::owner();
    let lease = claim(&mut owner);
    let snapshot = recover(&mut owner, lease);
    ack(&mut owner, lease, &snapshot).unwrap();
    owner
        .command(RequestId(20), Command::ReleaseControl { lease })
        .unwrap();
    assert!(owner.delivery.lease().is_none());
    assert_eq!(owner.phase, Phase::Active);
    assert!(
        matches!(owner.outgoing.pop_front(),Some(Event::ControllerRevoked{lease:actual,reason:ControllerRevocation::Released}) if actual==lease)
    );
    assert_eq!(
        ready(&mut owner, lease).unwrap_err().code,
        ErrorCode::ControlRequired
    );
    let replacement = claim(&mut owner);
    assert!(replacement.epoch > lease.epoch);
    assert!(owner.layouts.controller_ready(replacement).is_err());
    let new = match owner
        .command(
            RequestId(21),
            Command::Claim {
                controller: 2,
                takeover: true,
            },
        )
        .unwrap()
    {
        Response::Claimed(lease) => lease,
        _ => panic!(),
    };
    assert!(
        matches!(owner.outgoing.pop_front(),Some(Event::ControllerRevoked{lease:actual,reason:ControllerRevocation::Replaced}) if actual==replacement)
    );
    assert!(new.epoch > replacement.epoch);
    assert_eq!(owner.delivery.current_sequence(), 0);
}
fn mouse() -> shellfox_ghostty_vt::MouseEvent {
    shellfox_ghostty_vt::MouseEvent {
        action: shellfox_ghostty_vt::MouseAction::Press,
        button: Some(shellfox_ghostty_vt::MouseButton::Left),
        modifiers: shellfox_ghostty_vt::Modifiers::default(),
        x: 10.0,
        y: 20.0,
        any_button_pressed: true,
    }
}
