use super::*;
use shellfox_runtime::{Lease, ObserverLease};
fn controller() -> PresentationLease {
    PresentationLease::Controller(Lease {
        controller: 3,
        epoch: 1,
    })
}
fn observer() -> PresentationLease {
    PresentationLease::Observer(ObserverLease {
        viewer: 4,
        epoch: 2,
    })
}
fn guarded(version: u64) -> PresentationAction {
    PresentationAction::SelectionAtVersion {
        version,
        selection: Some(Selection::All),
    }
}
#[test]
fn guarded_selection_keeps_the_sampled_version_for_controller_and_observer() {
    for view in [controller(), observer()] {
        let action = checked_selection(view, view, view, 9, guarded(9), 80, 24).unwrap();
        assert!(matches!(
            action,
            PresentationAction::SelectionAtVersion {
                version: 9,
                selection: Some(Selection::All)
            }
        ));
    }
}
#[test]
fn stale_lease_display_and_version_are_rejected_not_retargeted() {
    assert!(
        checked_selection(controller(), observer(), observer(), 9, guarded(9), 80, 24).is_err()
    );
    assert!(
        checked_selection(observer(), observer(), controller(), 9, guarded(9), 80, 24).is_err()
    );
    assert!(checked_selection(observer(), observer(), observer(), 10, guarded(9), 80, 24).is_err());
}
#[test]
fn unguarded_or_other_actions_and_outside_coordinates_are_not_authorized() {
    for action in [
        PresentationAction::Selection { selection: None },
        PresentationAction::Copy,
        PresentationAction::FollowTail,
        PresentationAction::Frame { focused: true },
        PresentationAction::Scroll { delta: 1 },
    ] {
        assert!(checked_selection(observer(), observer(), observer(), 9, action, 80, 24).is_err());
    }
    let action = PresentationAction::SelectionAtVersion {
        version: 9,
        selection: Some(Selection::Word(ViewportPoint { x: 80, y: 0 })),
    };
    assert!(checked_selection(observer(), observer(), observer(), 9, action, 80, 24).is_err());
}
