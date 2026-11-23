#![cfg(windows)]
#[path = "activity_integration_support.rs"]
pub mod support;
use shellfox_runtime::{Command, Event, LiveOutputStatus, Phase, Response};
use shellfox_service::activity::{Health, SessionActivity, SessionAging};
use std::time::{Duration, Instant};
use support::Fixture;

#[test]
fn live_output_touches_inactivity_without_any_controller_and_does_not_run_for_screen_reads() {
    let (mut fixture, _, policy) = Fixture::start("--child-flood");
    let deadline = Instant::now() + Duration::from_secs(10);
    while fixture.runtime().phase() != Phase::Closed {
        assert!(
            Instant::now() < deadline,
            "unattached owned flood did not finish"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    while let Ok(event) = fixture.runtime().try_recv() {
        assert!(
            !matches!(event, Event::Output { .. }),
            "no view/controller was ever attached"
        );
    }
    assert_eq!(
        fixture.runtime().live_output_status(),
        LiveOutputStatus::Active
    );
    let updates = policy.take_live_updates();
    assert_eq!(updates.len(), 1);
    let update = updates[0];
    assert!(update.last_sequence > 32);
    assert_eq!(update.busy_changed, Some(true));
    assert!(update.available);
    let mut session = SessionActivity::new();
    session.touch(update.output_at_ms.unwrap());
    assert_eq!(
        session.last_action_ms(SessionAging {
            updated_ms: 0,
            settled_ms: None,
            pinned: false
        }),
        update.output_at_ms.unwrap()
    );
    let _ = fixture.runtime().take_screen();
    assert!(policy.take_live_updates().is_empty());
    let lease = match fixture.request(Command::Claim {
        controller: 19,
        takeover: false,
    }) {
        Response::Claimed(lease) => lease,
        _ => panic!("missing post-exit replay lease"),
    };
    assert!(
        matches!(fixture.request(Command::Replay { lease, after: update.last_sequence - 1 }), Response::Replay(chunks) if !chunks.is_empty())
    );
    assert!(matches!(
        fixture.request(Command::RecoverAnsi { lease }),
        Response::Ansi { .. }
    ));
    assert!(matches!(
        fixture.request(Command::Screen { lease }),
        Response::Applied
    ));
    assert!(
        policy.take_live_updates().is_empty(),
        "replay/recovery/render must not refresh activity"
    );
    fixture.close();
}

#[test]
fn blocked_ack_does_not_block_live_busy_or_ttl_and_meaningful_output_while_busy() {
    let (mut fixture, _, mut policy) = Fixture::start("--child-flood-on-input");
    let lease = match fixture.request(Command::Claim {
        controller: 29,
        takeover: false,
    }) {
        Response::Claimed(lease) => lease,
        _ => panic!("missing flood fixture lease"),
    };
    if let Response::Replay(chunks) = fixture.request(Command::Replay { lease, after: 0 }) {
        for chunk in chunks {
            fixture.output.extend(chunk.bytes);
        }
    }
    fixture.until_output("RUNTIME_FLOOD_READY");
    let initial = policy.take_live_updates().remove(0);
    // This is a fixture protocol trigger, not captured human input. No input TTL
    // touch or echo marker is supplied; all following touches must be live output.
    let admission = shellfox_service::activity::integration::InputAdmission::raw(
        b"start\r",
        false,
        false,
        shellfox_host::CapturedSettlement::Legacy,
    )
    .unwrap();
    let (response, accepted) = fixture.request_with_context(
        Command::RawInput {
            lease,
            bytes: b"start\r".to_vec(),
        },
        Some(admission),
    );
    assert!(matches!(response, Response::Input(receipt) if receipt.accepted_bytes == 6));
    assert!(!accepted.unwrap().activity.touch_session);
    let deadline = Instant::now() + Duration::from_secs(10);
    while fixture.runtime().phase() != Phase::Closed {
        assert!(
            Instant::now() < deadline,
            "unacknowledged flood blocked parser/cleanup"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let update = policy.take_live_updates().remove(0);
    assert!(update.last_sequence > initial.last_sequence + 32);
    assert!(update.output_at_ms.is_some());
    assert!(update.meaningful_at_ms.is_some());
    assert_eq!(
        update.busy_changed, None,
        "work while already busy still has timestamps"
    );
    assert!(policy.terminal_facts(fixture.identity, Phase::Closed).busy);
    assert!(policy.confirmed_exit(fixture.identity).touch_session);
    assert!(!policy.confirmed_exit(fixture.identity).touch_session);
    fixture.close();
}

#[test]
fn native_row_motion_keeps_the_actual_legacy_meaningful_classification() {
    let (mut fixture, _, policy) = Fixture::start("--child-idle-escapes-repeat");
    let sink = policy.live_output_sink(fixture.identity).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while sink.last_sequence().unwrap_or(0) < 4 {
        assert!(Instant::now() < deadline, "owned row feeds did not start");
        std::thread::sleep(Duration::from_millis(2));
    }
    policy.take_live_updates();
    let after = sink.last_sequence().unwrap();
    let previous_meaningful = sink.last_meaningful_output_ms();
    while sink.last_sequence().unwrap() < after + 4 {
        assert!(
            Instant::now() < deadline,
            "continuing owned row feeds stopped"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let update = policy.take_live_updates().remove(0);
    let lease = match fixture.request(Command::Claim {
        controller: 31,
        takeover: false,
    }) {
        Response::Claimed(lease) => lease,
        _ => panic!("missing diagnostic replay lease"),
    };
    let chunks = match fixture.request(Command::Replay { lease, after }) {
        Response::Replay(chunks) => chunks,
        _ => panic!("missing controlled replay"),
    };
    let bytes: usize = chunks.iter().map(|chunk| chunk.bytes.len()).sum();
    assert!(bytes <= 128 * 1024);
    let line_break = chunks
        .iter()
        .any(|chunk| chunk.bytes.contains(&b'\r') || chunk.bytes.contains(&b'\n'));
    let sgr = chunks
        .iter()
        .any(|chunk| chunk.bytes.windows(3).any(|bytes| bytes == b"\x1b[m"));
    let erase = chunks
        .iter()
        .any(|chunk| chunk.bytes.windows(4).any(|bytes| bytes == b"\x1b[2J"));
    eprintln!("owned row fixture coarse diagnostics: bytes={bytes}, line_break={line_break}, sgr_reset={sgr}, clear_screen={erase}");
    assert!(update.meaningful_at_ms.is_some());
    assert!(sink.last_meaningful_output_ms() > previous_meaningful);
    assert_eq!(sink.status(), (true, true));
    fixture.close();
}

#[test]
fn native_visibility_toggles_refresh_ttl_without_busy_after_honest_startup() {
    let (mut fixture, _, policy) = Fixture::start("--child-visibility-escapes-repeat");
    let sink = policy.live_output_sink(fixture.identity).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while sink.last_sequence().unwrap_or(0) == 0 {
        assert!(
            Instant::now() < deadline,
            "owned idle escapes were not observed"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let initial = policy.take_live_updates().remove(0);
    // ConPTY startup clear/normalization is real meaningful output. Do not filter
    // it away: wait for its quiet period while fresh cursor-only feeds continue.
    loop {
        assert!(
            Instant::now() < deadline,
            "continuing idle feeds never became quiet"
        );
        policy.tick(policy.clock().now_ms().unwrap());
        if sink.status() == (false, true) && sink.last_sequence().unwrap() > initial.last_sequence {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let quiet = policy.take_live_updates().remove(0);
    let meaningful = sink.last_meaningful_output_ms();
    let after = sink.last_sequence().unwrap();
    while sink.last_sequence().unwrap() <= after {
        assert!(
            Instant::now() < deadline,
            "idle stream stopped instead of refreshing TTL"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let update = policy.take_live_updates().remove(0);
    assert!(update.output_at_ms > quiet.output_at_ms);
    assert_eq!(update.meaningful_at_ms, None);
    assert_eq!(update.busy_changed, None);
    assert_eq!(sink.last_meaningful_output_ms(), meaningful);
    assert_eq!(sink.status(), (false, true));
    let mut session = SessionActivity::new();
    session.touch(update.output_at_ms.unwrap());
    assert_eq!(
        session.last_action_ms(SessionAging {
            updated_ms: 0,
            settled_ms: None,
            pinned: false
        }),
        update.output_at_ms.unwrap()
    );
    fixture.close();
}

#[test]
fn optional_service_sink_failure_keeps_the_real_pty_working_and_fails_expiry_closed() {
    let (mut fixture, _, mut policy) = Fixture::start("--child-echo");
    let lease = match fixture.request(Command::Claim {
        controller: 23,
        takeover: false,
    }) {
        Response::Claimed(lease) => lease,
        _ => panic!("missing echo fixture lease"),
    };
    if let Response::Replay(chunks) = fixture.request(Command::Replay { lease, after: 0 }) {
        for chunk in chunks {
            fixture.output.extend(chunk.bytes);
        }
    }
    fixture.until_output("RUNTIME_READY");
    let identity = fixture.identity;
    let since = policy.clock().now_ms().unwrap();
    policy.begin_expiry_refresh(&[identity], since);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !policy.fresh_healthy(&[identity], since) {
        assert!(Instant::now() < deadline, "owned echo refresh deadline");
        policy.poll_processes(
            &fixture.runtimes,
            &[],
            policy.clock().now_ms().unwrap(),
            1,
            true,
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    policy.live_output_sink(identity).unwrap().disable();
    assert!(matches!(
        fixture.request(Command::RawInput {
            lease,
            bytes: b"still-working\r".to_vec()
        }),
        Response::Input(receipt) if receipt.accepted_bytes == 14
    ));
    fixture.until_output("RUNTIME_ECHO=still-working");
    assert_eq!(
        fixture.runtime().live_output_status(),
        LiveOutputStatus::Unavailable
    );
    policy.sync_live_status(&fixture.runtimes);
    let facts = policy.terminal_facts(identity, fixture.runtime().phase());
    assert!(!facts.activity_available);
    assert_eq!(facts.observation.unwrap().health, Health::Unknown);
    assert!(!policy.fresh_healthy(&[identity], since));
    assert!(policy
        .take_live_updates()
        .iter()
        .any(|update| !update.available));
    fixture.close();
}
