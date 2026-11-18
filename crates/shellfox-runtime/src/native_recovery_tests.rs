//! Actual Owner/delivery/layout/VT boundary in the existing in-memory Owner fixture.
//! Not PTY process/launch privilege or cross-platform acceptance.
use super::*;
use shellfox_ghostty_vt::extension::{RestorationConfig, SnapshotLimits, VerifiedNativeExtension};
fn native_owner() -> Owner {
    let mut owner = super::test_support::owner();
    let config = crate::Native0RuntimeConfig::new(
        RestorationConfig {
            runtime_image_storage_limit: 10_000_000,
            runtime_resize_pull_scrollback: false,
        },
        SnapshotLimits::default(),
    )
    .unwrap();
    let (terminal, creation) = crate::recovery::Native0Creation::create(80, 24, config).unwrap();
    owner.terminal = terminal;
    owner.native_creation = Some(creation);
    owner
}
fn claim(owner: &mut Owner, controller: u64) -> Lease {
    let Response::Claimed(lease) = owner
        .command(
            RequestId(1),
            Command::Claim {
                controller,
                takeover: false,
            },
        )
        .unwrap()
    else {
        panic!("actual claim")
    };
    lease
}
fn recover(owner: &mut Owner, lease: Lease, id: u64) -> crate::StampedNative0Recovery {
    let Response::Native0Recovery(value) = owner
        .command(RequestId(id), Command::RecoverNative0Stamped { lease })
        .unwrap()
    else {
        panic!("actual Native0 recovery")
    };
    *value
}
#[test]
fn native0_capture_uses_real_lease_sequence_geometry_and_replacement_opaque_fence() {
    let mut owner = native_owner();
    let lease = claim(&mut owner, 17);
    owner
        .accept_source_bytes(b"primary\x1b[2;2H\x1b7\x1b[?1049halt\x1b[38;2;")
        .unwrap();
    let first = recover(&mut owner, lease, 2);
    let second = recover(&mut owner, lease, 3);
    assert_eq!(
        second.cut.last_delivery_sequence,
        owner.delivery.current_sequence()
    );
    assert_eq!((second.cut.cols, second.cut.rows), (80, 24));
    assert_ne!(first.cut.recovery_id, second.cut.recovery_id);
    assert!(owner
        .command(
            RequestId(4),
            Command::Ack {
                lease,
                sequence: second.cut.last_delivery_sequence
            }
        )
        .is_err());
    assert!(owner
        .command(
            RequestId(5),
            Command::ControllerLayoutAck {
                lease,
                recovery_id: first.cut.recovery_id,
                last_delivery_sequence: first.cut.last_delivery_sequence,
                layout_revision: first.cut.layout_revision
            }
        )
        .is_err());
    assert!(owner
        .command(RequestId(6), Command::CheckControllerReady { lease })
        .is_err());
    let verified = VerifiedNativeExtension::verify().unwrap();
    let (mut passive, _) = second.snapshot.decode(&verified, second.limits).unwrap();
    for bytes in [b"10;20;30m!".as_slice(), b"\x1b[?1049l\x1b8Y".as_slice()] {
        owner.accept_source_bytes(bytes).unwrap();
        passive.feed(bytes).unwrap();
        let actual = owner.terminal.render().unwrap();
        let restored = passive.render().unwrap();
        assert_eq!(actual.cells, restored.cells);
        assert_eq!(actual.cursor, restored.cursor);
        assert_eq!(
            owner.terminal.continuation().unwrap(),
            passive.continuation().unwrap()
        );
    }
    assert!(matches!(
        owner
            .command(
                RequestId(7),
                Command::ControllerLayoutAck {
                    lease,
                    recovery_id: second.cut.recovery_id,
                    last_delivery_sequence: second.cut.last_delivery_sequence,
                    layout_revision: second.cut.layout_revision
                }
            )
            .unwrap(),
        Response::Applied
    ));
    assert!(matches!(
        owner
            .command(RequestId(8), Command::CheckControllerReady { lease })
            .unwrap(),
        Response::Applied
    ));
}
#[test]
fn default_source_is_not_relabelled_native0_and_failed_capture_preserves_old_view() {
    let mut owner = super::test_support::owner();
    let lease = claim(&mut owner, 18);
    owner
        .accept_source_bytes(b"existing ordinary source")
        .unwrap();
    let before = owner.terminal.render().unwrap();
    assert_eq!(
        owner
            .command(RequestId(2), Command::RecoverNative0Stamped { lease })
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    assert_eq!(owner.terminal.render().unwrap().cells, before.cells);
    assert!(!owner.layouts.controller_opted(lease));
}
#[test]
fn native0_recovery_cannot_restore_authority_after_actual_release() {
    let mut owner = native_owner();
    let lease = claim(&mut owner, 19);
    let recovery = recover(&mut owner, lease, 2);
    owner
        .command(RequestId(3), Command::ReleaseControl { lease })
        .unwrap();
    assert!(owner
        .command(RequestId(4), Command::RecoverNative0Stamped { lease })
        .is_err());
    assert!(owner
        .command(
            RequestId(5),
            Command::ControllerLayoutAck {
                lease,
                recovery_id: recovery.cut.recovery_id,
                last_delivery_sequence: recovery.cut.last_delivery_sequence,
                layout_revision: recovery.cut.layout_revision
            }
        )
        .is_err());
}
