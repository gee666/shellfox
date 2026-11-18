use super::*;
fn observer() -> ObserverLease {
    ObserverLease {
        viewer: 1,
        epoch: 1,
    }
}
fn initial(layouts: &mut Layouts, sequence: u64) -> StampedRecovery {
    let snapshot = layouts
        .recovery(
            observer(),
            vec![1],
            AnsiRecoveryCapability::VisibleScreen,
            sequence,
        )
        .unwrap();
    layouts
        .validate_ack(
            observer(),
            snapshot.recovery_id,
            sequence,
            snapshot.layout_revision,
        )
        .unwrap();
    layouts.accepted(observer());
    snapshot
}
#[test]
fn actual_geometry_only_advances_revision_not_bytes_or_duplicate_resize() {
    let mut layouts = Layouts::new(80, 24);
    layouts.changed(80, 24, 900).unwrap();
    assert_eq!(layouts.current.layout_revision, 1);
    layouts.changed(100, 30, 900).unwrap();
    assert_eq!(layouts.current.layout_revision, 2);
    layouts.changed(100, 30, 999).unwrap();
    assert_eq!(layouts.current.layout_revision, 2);
}
#[test]
fn identical_sequence_and_layout_still_require_the_latest_opaque_recovery_id() {
    let mut layouts = Layouts::new(80, 24);
    let old = layouts
        .recovery(
            observer(),
            vec![1],
            AnsiRecoveryCapability::VisibleScreen,
            4,
        )
        .unwrap();
    let new = layouts
        .recovery(
            observer(),
            vec![2],
            AnsiRecoveryCapability::VisibleScreen,
            4,
        )
        .unwrap();
    assert_ne!(old.recovery_id, new.recovery_id);
    assert!(layouts
        .validate_ack(observer(), old.recovery_id, 4, 1)
        .is_err());
    assert!(!layouts.data_ack_allowed(observer()));
    layouts
        .validate_ack(observer(), new.recovery_id, 4, 1)
        .unwrap();
    layouts.accepted(observer());
    assert!(layouts.data_ack_allowed(observer()));
}
#[test]
fn layout_notification_follows_prior_bytes_and_never_grants_resize_only_acceptance() {
    let mut layouts = Layouts::new(80, 24);
    let old = initial(&mut layouts, 0);
    layouts.changed(100, 30, 2).unwrap();
    for next in [1, 2] {
        assert!(layouts.allows(observer(), next, 5, |_| panic!(
            "prior bytes must come first"
        )));
    }
    let mut notification = None;
    assert!(!layouts.allows(observer(), 3, 5, |event| {
        notification = Some(event);
        true
    }));
    assert!(matches!(
        notification,
        Some(LayoutNotice::Changed(LayoutStamp {
            last_delivery_sequence: 2,
            cols: 100,
            rows: 30,
            layout_revision: 2
        }))
    ));
    assert!(
        layouts
            .validate_ack(observer(), old.recovery_id, 2, 2)
            .is_err(),
        "a layout notification is not a recovery grant"
    );
    assert!(!layouts.allows(observer(), 3, 5, |_| panic!("must wait for full recovery")));
    let new = layouts
        .recovery(
            observer(),
            vec![9],
            AnsiRecoveryCapability::FormattedActiveBufferV1,
            5,
        )
        .unwrap();
    assert_eq!(
        (
            new.cols,
            new.rows,
            new.layout_revision,
            new.last_delivery_sequence
        ),
        (100, 30, 2, 5)
    );
    layouts
        .validate_ack(observer(), new.recovery_id, 5, 2)
        .unwrap();
    layouts.accepted(observer());
    assert!(layouts.allows(observer(), 6, 5, |_| panic!("latest layout accepted")));
}
#[test]
fn resize_after_snapshot_cannot_escape_when_that_older_snapshot_is_accepted() {
    let mut layouts = Layouts::new(80, 24);
    let old = layouts
        .recovery(
            observer(),
            vec![1],
            AnsiRecoveryCapability::VisibleScreen,
            4,
        )
        .unwrap();
    layouts.changed(100, 30, 4).unwrap();
    layouts
        .validate_ack(observer(), old.recovery_id, 4, 1)
        .unwrap();
    layouts.accepted(observer());
    assert!(!layouts.allows(observer(), 5, 5, |event| matches!(
        event,
        LayoutNotice::Changed(_)
    )));
}
#[test]
fn bounded_geometry_journal_overrun_requires_recovery_without_skipping_or_notifying_twice() {
    let mut layouts = Layouts::new(80, 24);
    initial(&mut layouts, 0);
    for revision in 0..65 {
        layouts.changed(81 + revision, 24, revision as u64).unwrap();
    }
    assert_eq!(layouts.changes.len(), CHANGES);
    assert!(!layouts.allows(observer(), 1, 99, |event| matches!(
        event,
        LayoutNotice::Lost(99)
    )));
    assert!(!layouts.allows(observer(), 1, 99, |_| panic!(
        "no duplicate loss before recovery"
    )));
    let new = layouts
        .recovery(
            observer(),
            vec![9],
            AnsiRecoveryCapability::VisibleScreen,
            99,
        )
        .unwrap();
    layouts
        .validate_ack(observer(), new.recovery_id, 99, new.layout_revision)
        .unwrap();
    layouts.accepted(observer());
    assert!(layouts.allows(observer(), 100, 99, |_| panic!(
        "latest recovery initializes current geometry"
    )));
}
#[test]
fn full_event_channel_does_not_commit_layout_notification_or_bypass_recovery() {
    let mut layouts = Layouts::new(80, 24);
    initial(&mut layouts, 0);
    layouts.changed(100, 30, 0).unwrap();
    for _ in 0..3 {
        assert!(!layouts.allows(observer(), 1, 1, |_| false));
    }
    let mut notices = 0;
    assert!(!layouts.allows(observer(), 1, 1, |_| {
        notices += 1;
        true
    }));
    assert_eq!(notices, 1);
}
#[test]
fn ordinary_recovery_id_or_domain_failure_cannot_replace_a_prior_pending_token() {
    let mut layouts = Layouts::new(80, 24);
    let original = layouts
        .recovery(
            observer(),
            vec![1],
            AnsiRecoveryCapability::VisibleScreen,
            3,
        )
        .unwrap();
    let counter = layouts.next_recovery;
    layouts.next_recovery = u64::MAX;
    assert!(layouts
        .recovery(
            observer(),
            vec![2],
            AnsiRecoveryCapability::VisibleScreen,
            4
        )
        .is_err());
    assert_eq!(layouts.next_recovery, u64::MAX);
    layouts
        .validate_ack(observer(), original.recovery_id, 3, 1)
        .unwrap();
    layouts.next_recovery = counter;
    layouts.domain = None;
    assert!(layouts
        .recovery(
            observer(),
            vec![2],
            AnsiRecoveryCapability::VisibleScreen,
            4
        )
        .is_err());
    assert_eq!(layouts.next_recovery, counter);
    layouts
        .validate_ack(observer(), original.recovery_id, 3, 1)
        .unwrap();
    assert!(!layouts.data_ack_allowed(observer()));
}
