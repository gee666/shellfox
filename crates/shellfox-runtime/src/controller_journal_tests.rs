use super::*;
#[test]
fn resize_after_snapshot_or_journal_overrun_never_releases_current_side_effect_gate() {
    let mut owner = test_support::owner();
    let lease = claim(&mut owner);
    let snapshot = recover(&mut owner, lease);
    owner.canonical_resize(100, 30).unwrap();
    ack(&mut owner, lease, &snapshot).unwrap();
    assert_eq!(
        ready(&mut owner, lease).unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    for cols in 101..171 {
        owner.canonical_resize(cols, 30).unwrap();
    }
    let (send, receive) = crate::owner_wake::channel(&owner.shared, 1);
    owner.emit_events(&send);
    assert!(
        matches!(receive.try_recv().unwrap(),Event::ReplayLost{lease:actual,..} if actual==lease)
    );
    let latest = recover(&mut owner, lease);
    assert_eq!((latest.cols, latest.rows), (170, 30));
    ack(&mut owner, lease, &latest).unwrap();
    assert!(ready(&mut owner, lease).is_ok());
}
