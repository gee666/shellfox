use super::*;
use crate::{activity::*, activity_integration::ActivityUtcAnchor};
use shellfox_core::{Generation, SessionId, TabId};
use std::{sync::Arc, time::Duration};
fn identity() -> Identity {
    Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    }
}
fn fixture() -> (Arc<LiveActivitySink>, Arc<ActivityClock>) {
    let clock = Arc::new(ActivityClock::new(Instant::now(), 0));
    (
        Arc::new(LiveActivitySink::new(identity(), Arc::clone(&clock))),
        clock,
    )
}
#[test]
fn idle_and_echo_refresh_ttl_but_not_busy_and_meaningful_busy_output_has_its_own_timestamp() {
    let (sink, clock) = fixture();
    sink.observe(
        sink.identity,
        1,
        b"\x1b[?2026h\x1b[35;1H\x1b[?25l\x1b[?2026l",
        clock.instant_at(100).unwrap(),
    )
    .unwrap();
    let idle = sink.take_update().unwrap();
    assert_eq!(idle.output_at_ms, Some(100));
    assert_eq!(idle.meaningful_at_ms, None);
    assert_eq!(idle.busy_changed, None);
    let mut ttl = SessionActivity::new();
    ttl.touch(idle.output_at_ms.unwrap());
    assert_eq!(
        ttl.last_action_ms(SessionAging {
            updated_ms: 0,
            settled_ms: None,
            pinned: false
        }),
        100
    );
    sink.register_input(
        sink.identity,
        RequestId(1),
        InputAdmission::raw(b"x", true, true, shellfox_host::CapturedSettlement::Legacy).unwrap(),
    )
    .unwrap();
    shellfox_runtime::LiveOutputSink::before_input(
        sink.as_ref(),
        sink.identity,
        RequestId(1),
        InputKind::Raw,
        b"x",
        clock.instant_at(200).unwrap(),
    )
    .unwrap();
    let receipt = Ok(Response::Input(shellfox_runtime::InputReceipt {
        accepted_bytes: 1,
        kind: InputKind::Raw,
    }));
    sink.finish_input(sink.identity, RequestId(1), &receipt)
        .unwrap();
    sink.observe(sink.identity, 2, b"echo", clock.instant_at(200).unwrap())
        .unwrap();
    let echo = sink.take_update().unwrap();
    assert_eq!(echo.output_at_ms, Some(200));
    assert_eq!(echo.meaningful_at_ms, None);
    assert_eq!(echo.busy_changed, None);
    sink.observe(sink.identity, 3, b"work", clock.instant_at(450).unwrap())
        .unwrap();
    let work = sink.take_update().unwrap();
    assert_eq!(work.meaningful_at_ms, Some(450));
    assert_eq!(work.busy_changed, Some(true));
    sink.observe(
        sink.identity,
        4,
        b"more work",
        clock.instant_at(600).unwrap(),
    )
    .unwrap();
    let more = sink.take_update().unwrap();
    assert_eq!(more.meaningful_at_ms, Some(600));
    assert_eq!(more.busy_changed, None);
    assert_eq!(sink.last_meaningful_output_ms(), Some(600));
    sink.observe(sink.identity, 5, b"\x1b[H", clock.instant_at(700).unwrap())
        .unwrap();
    let idle = sink.take_update().unwrap();
    assert_eq!(idle.output_at_ms, Some(700));
    assert_eq!(idle.meaningful_at_ms, None);
    assert_eq!(sink.last_meaningful_output_ms(), Some(600));
}
#[test]
fn busy_expiry_and_read_only_status_do_not_invent_ttl_touches() {
    let (sink, clock) = fixture();
    sink.observe(sink.identity, 1, b"work", clock.instant_at(0).unwrap())
        .unwrap();
    sink.take_update();
    assert_eq!(sink.status(), (true, true));
    assert_eq!(sink.last_sequence(), Some(1));
    assert_eq!(sink.last_meaningful_output_ms(), Some(0));
    assert!(sink.take_update().is_none());
    assert_eq!(sink.tick(2500), Ok(Some(false)));
    let quiet = sink.take_update().unwrap();
    assert_eq!(quiet.output_at_ms, None);
    assert_eq!(quiet.meaningful_at_ms, None);
    assert_eq!(quiet.busy_changed, Some(false));
}
#[test]
fn duplicate_generation_or_poison_disables_only_the_observer() {
    let (sink, clock) = fixture();
    sink.observe(sink.identity, 1, b"work", clock.instant_at(0).unwrap())
        .unwrap();
    assert!(sink
        .observe(sink.identity, 1, b"replay", clock.instant_at(1).unwrap())
        .is_err());
    assert_eq!(sink.status(), (false, false));
    assert!(!sink.take_update().unwrap().available);
    assert!(sink.take_update().is_none());
    let (sink, _) = fixture();
    let _ = std::panic::catch_unwind(|| {
        let _guard = sink.state.lock().unwrap();
        panic!("injected activity mutex poison");
    });
    assert_eq!(sink.status(), (false, false));
    assert!(!sink.take_update().unwrap().available);
}
#[test]
fn poisoned_optional_policy_does_not_lose_the_single_actor_admission_book() {
    let (sink, clock) = fixture();
    let marker =
        shellfox_host::CapturedSettlement::Captured(Some("2026-01-01T00:00:00.000Z".into()));
    sink.register_input(
        sink.identity,
        RequestId(7),
        InputAdmission::raw(b"x", true, false, marker.clone()).unwrap(),
    )
    .unwrap();
    let _ = std::panic::catch_unwind(|| {
        let _guard = sink.state.lock().unwrap();
        panic!("injected optional filter poison");
    });
    assert!(shellfox_runtime::LiveOutputSink::before_input(
        sink.as_ref(),
        sink.identity,
        RequestId(7),
        InputKind::Raw,
        b"x",
        clock.instant_at(1).unwrap()
    )
    .is_err());
    let receipt = Ok(Response::Input(shellfox_runtime::InputReceipt {
        accepted_bytes: 1,
        kind: InputKind::Raw,
    }));
    let accepted = sink
        .finish_input(sink.identity, RequestId(7), &receipt)
        .unwrap()
        .unwrap();
    assert_eq!(accepted.admission.captured_settlement, marker);
    assert!(!accepted.admission.hint);
    assert!(accepted.activity.touch_session);
    assert_eq!(sink.status(), (false, false));
}

#[test]
fn instant_anchor_and_explicit_utc_mapping_share_one_checked_timeline() {
    let anchor = Instant::now();
    let clock = ActivityClock::new(anchor, 1000);
    assert_eq!(clock.at(anchor + Duration::from_millis(250)), Some(1250));
    assert_eq!(clock.at(anchor - Duration::from_millis(1)), Some(999));
    assert_eq!(
        clock.instant_at(1250),
        Some(anchor + Duration::from_millis(250))
    );
    let utc = ActivityUtcAnchor {
        logical_ms: 1000,
        unix_ms: 1_700_000_000_000,
    };
    assert_eq!(utc.to_unix_ms(1250), Some(1_700_000_000_250));
    assert_eq!(utc.from_unix_ms(1_699_999_999_999), Some(999));
    assert_eq!(utc.to_unix_ms(u64::MAX), None);
}
