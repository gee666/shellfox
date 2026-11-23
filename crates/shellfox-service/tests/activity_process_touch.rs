use shellfox_core::{Generation, SessionId, TabId};
use shellfox_runtime::{Identity, LiveOutputSink, Runtime};
use shellfox_service::activity::integration::{InspectionScope, OwnerActivity};
use std::collections::HashMap;
#[test]
fn unsupported_first_poll_and_silence_are_diagnostic_not_ttl_but_all_live_output_still_touches() {
    let identity = Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    };
    let mut owner = OwnerActivity::new();
    owner
        .watch(identity, "raw-program".into(), InspectionScope::Unsupported)
        .unwrap();
    let source = HashMap::<TabId, Runtime>::new();
    let first = owner.poll_processes(&source, &[], 0, 1, false);
    assert_eq!(first.len(), 1);
    assert!(first[0].activity_changed);
    assert!(!first[0].touch_session);
    assert!(owner.take_live_updates().is_empty());
    let sink = owner.live_output_sink(identity).unwrap();
    sink.output(
        identity,
        1,
        b"\x1b[?25l",
        owner.clock().instant_at(100).unwrap(),
    )
    .unwrap();
    assert_eq!(sink.take_update().unwrap().output_at_ms, Some(100));
    let stable = owner.poll_processes(&source, &[], 1000, 1, false);
    assert!(!stable[0].activity_changed);
    assert!(!stable[0].touch_session);
    assert!(owner.tick(5000).is_empty());
    assert!(owner.take_live_updates().is_empty());
    assert!(owner.confirmed_exit(identity).touch_session);
    let after = owner.poll_processes(&source, &[], 2000, 1, true);
    assert!(!after[0].touch_session);
    assert!(!owner.confirmed_exit(identity).touch_session);
}
