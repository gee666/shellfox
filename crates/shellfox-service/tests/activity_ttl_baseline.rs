use shellfox_core::{Generation, SessionId, Settings, TabId};
use shellfox_runtime::{Identity, Phase};
use shellfox_service::{
    activity::integration::{ActivityClock, InspectionScope, OwnerActivity},
    activity::{SessionActivity, SessionAging},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
fn idle_live_output_refreshes_ttl_without_becoming_busy() {
    let base = Instant::now();
    let mut owner = OwnerActivity::with_clock(Arc::new(ActivityClock::new(base, 0)));
    let identity = Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    };
    owner
        .watch(identity, "fixture".into(), InspectionScope::Unsupported)
        .unwrap();
    let sink = owner.live_output_sink(identity).unwrap();
    let settings = Settings {
        session_settle_hours: 1.0,
        ..Default::default()
    };
    let session = SessionAging {
        updated_ms: 0,
        settled_ms: None,
        pinned: false,
    };
    let now = 3_600_001;
    let at = now - 2;
    assert!(SessionActivity::new().due(session, &settings, now));

    // This is the authoritative live hook, not view delivery or a replay copy.
    sink.observe(
        identity,
        1,
        b"\x1b[?25l\x1b[2;3H",
        base + Duration::from_millis(at),
    )
    .unwrap();
    let updates = owner.take_live_updates();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].output_at_ms, Some(at));
    assert_eq!(updates[0].busy_changed, None);
    assert!(!sink.status().0);
    let mut activity = SessionActivity::new();
    activity.touch(updates[0].output_at_ms.unwrap());
    assert_eq!(activity.pending_update(0), Some(at));
    assert!(!activity.due(session, &settings, now));

    // Reading view/fact state is not another output feed or persistence touch.
    for _ in 0..3 {
        let _ = sink.status();
        let _ = owner.terminal_facts(identity, Phase::Active);
    }
    assert!(owner.take_live_updates().is_empty());
    assert_eq!(activity.pending_update(0), Some(at));
    assert!(owner.confirmed_exit(identity).touch_session);
    assert!(!owner.confirmed_exit(identity).touch_session);
}
