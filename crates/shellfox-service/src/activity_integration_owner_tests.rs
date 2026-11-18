use super::*;
use shellfox_core::{Generation, SessionId};
#[test]
fn confirmed_exit_blocks_even_a_changed_authoritative_poll_without_hiding_diagnostics() {
    let identity = Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    };
    let mut owner = OwnerActivity::new();
    owner
        .watch(
            identity,
            "actual-program".into(),
            InspectionScope::LocalWindows,
        )
        .unwrap();
    let mut updates = vec![ProcessUpdate {
        identity,
        observation: Observation {
            root: RootState::Alive,
            health: Health::Healthy,
            agents: 1,
            reason: None,
            observed_at_ms: 0,
        },
        activity_changed: true,
        touch_session: true,
    }];
    owner.apply_exit_barrier(&mut updates);
    assert!(updates[0].touch_session);
    assert!(owner.confirmed_exit(identity).touch_session);
    owner.apply_exit_barrier(&mut updates);
    assert!(!updates[0].touch_session);
    assert!(updates[0].activity_changed);
    assert!(
        !owner.confirmed_exit(identity).touch_session,
        "confirmed exit is never replayed by a poll"
    );
}
