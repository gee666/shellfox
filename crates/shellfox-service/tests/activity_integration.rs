#![cfg(windows)]
#[path = "activity_integration_support.rs"]
pub mod support;
use shellfox_core::{
    CreateSession, Generation, ProfileId, SessionId, SettingsProcessRule, TerminalIdentity,
};
use shellfox_runtime::{Command, Phase, Response};
use shellfox_service::{activity::*, activity::integration::*};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use support::Fixture;

#[test]
fn actual_owned_job_inspection_retains_controller_detects_agent_and_requires_a_new_refresh() {
    let (mut fixture, executable, mut policy) = Fixture::start("--child-inspection");
    let lease = match fixture.request(Command::Claim {
        controller: 17,
        takeover: false,
    }) {
        Response::Claimed(lease) => lease,
        _ => panic!("missing control lease"),
    };
    if let Response::Replay(chunks) = fixture.request(Command::Replay { lease, after: 0 }) {
        for chunk in chunks {
            fixture.output.extend(chunk.bytes);
        }
    }
    fixture.until_output("OWNED_AGENT_READY");
    let identity = fixture.identity;
    let mut state = RuntimeInspectionState::new();
    state
        .watch(identity, InspectionScope::LocalWindows)
        .unwrap();
    let request_at = policy.clock().now_ms().unwrap();
    assert!(matches!(
        RuntimeProcessInspector::new(&fixture.runtimes, &mut state, request_at).inspect(identity),
        Err(InspectionFailure::Pending)
    ));
    assert_eq!(
        fixture.runtime().request_inspection(),
        Err(shellfox_runtime::InspectionFailure::Pending)
    );
    let refresh_at = request_at + 1;
    state.require_fresh(&[identity], refresh_at);
    assert!(!state.fresh_since(&[identity], refresh_at));
    let deadline = Instant::now() + Duration::from_secs(10);
    let evidence = loop {
        assert!(Instant::now() < deadline, "owned inspection deadline");
        let now = policy.clock().now_ms().unwrap();
        match RuntimeProcessInspector::new(&fixture.runtimes, &mut state, now).inspect(identity) {
            Ok(snapshot) => break snapshot,
            Err(InspectionFailure::Pending) => std::thread::sleep(Duration::from_millis(2)),
            Err(error) => panic!("owned inspection failed: {error}"),
        }
    };
    assert!(state.fresh_since(&[identity], refresh_at));
    let root = evidence
        .processes
        .iter()
        .find(|process| process.is_root)
        .unwrap();
    assert!(root.identity.birth.parse::<u64>().unwrap() > 0);
    assert_ne!(root.identity.pid, std::process::id());
    assert!(root
        .argv
        .as_ref()
        .unwrap()
        .iter()
        .any(|arg| arg == "--child-inspection"));
    assert!(evidence.processes.iter().all(|process| !process
        .argv
        .as_ref()
        .is_some_and(|argv| argv.iter().any(|arg| arg == "--shellfox-pty-supervisor"))));
    let rule = SettingsProcessRule {
        id: "00000000-0000-4000-8000-000000000001".into(),
        label: "Controlled fixture agent".into(),
        enabled: true,
        executable_basenames: vec![executable.file_name().unwrap().to_str().unwrap().into()],
        executable_paths: vec![],
        script_path_suffixes: vec![],
        process_names: None,
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "owned agent detection deadline");
        let now = policy.clock().now_ms().unwrap();
        let updates =
            policy.poll_processes(&fixture.runtimes, std::slice::from_ref(&rule), now, 1, true);
        if updates.iter().any(|update| {
            update.observation.agents == 1 && update.observation.health == Health::Healthy
        }) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        policy
            .terminal_facts(identity, Phase::Active)
            .observation
            .unwrap()
            .agents,
        1
    );
    let admission = InputAdmission::raw(
        b"typing\r",
        true,
        true,
        shellfox_host::CapturedSettlement::Captured(None),
    )
    .unwrap();
    let (response, accepted) = fixture.request_with_context(
        Command::RawInput {
            lease,
            bytes: b"typing\r".to_vec(),
        },
        Some(admission),
    );
    assert!(
        matches!(response, Response::Input(receipt) if receipt.accepted_bytes == 7 && receipt.kind == shellfox_runtime::InputKind::Raw)
    );
    let accepted = accepted.unwrap();
    assert!(accepted.activity.touch_session);
    assert_eq!(
        accepted.admission.captured_settlement,
        shellfox_host::CapturedSettlement::Captured(None)
    );
    assert!(!should_unsettle(Some("new-settlement"), Some(None)));
    fixture.until_output("INSPECTION_TYPING=typing");
    assert!(policy
        .take_live_updates()
        .iter()
        .any(|update| update.output_at_ms.is_some() && update.available));
    let since = policy.clock().now_ms().unwrap() + 1;
    policy.begin_expiry_refresh(&[identity], since);
    assert!(!policy.fresh_healthy(&[identity], since));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !policy.fresh_healthy(&[identity], since) {
        assert!(Instant::now() < deadline, "fresh owned refresh deadline");
        let now = policy.clock().now_ms().unwrap();
        policy.poll_processes(&fixture.runtimes, std::slice::from_ref(&rule), now, 1, true);
        std::thread::sleep(Duration::from_millis(2));
    }
    fixture.close();
    policy.forget(identity);
    assert!(!policy.fresh_healthy(&[identity], since));
}

#[test]
fn real_registered_job_keeps_outbox_captured_markers_and_never_retries_metadata_failed_input() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root).unwrap();
    let scratch = tempfile::Builder::new()
        .prefix("activity-input-store-")
        .tempdir_in(root)
        .unwrap();
    let host = shellfox_host::Host::open(scratch.path()).unwrap();
    let snapshot = host
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: Some("receipt fixture".into()),
            cwd: scratch.path().to_string_lossy().into_owned(),
            profile_id: ProfileId::new("cmd").unwrap(),
        })
        .unwrap();
    let identity = shellfox_runtime::Identity {
        session_id: snapshot.selection.session_id.unwrap(),
        tab_id: snapshot.selection.tab_id.unwrap(),
        generation: Generation::new(),
    };
    let registered = TerminalIdentity {
        tab_id: identity.tab_id,
        generation: identity.generation,
    };
    host.register_terminal(registered).unwrap();
    let (mut fixture, _, mut policy) = Fixture::start_with_identity("--child-inspection", identity);
    let mut persistence = ActivityPersistence::new(ActivityUtcAnchor {
        logical_ms: 0,
        unix_ms: shellfox_host::activity_unix_ms("2050-01-01T00:00:00.000Z").unwrap(),
    });
    let lease = match fixture.request(Command::Claim {
        controller: 37,
        takeover: false,
    }) {
        Response::Claimed(lease) => lease,
        _ => panic!("missing registered fixture lease"),
    };
    // Capture known null at UI-intent admission, before the later manual settle.
    let null = InputAdmission::native(
        true,
        false,
        shellfox_host::CapturedSettlement::Captured(snapshot.sessions[0].settled_at.clone()),
    );
    let settled = host.set_session_settled(identity.session_id, true).unwrap();
    let first = settled.sessions[0].settled_at.clone();
    assert!(
        host.register_terminal(registered).is_err(),
        "manual settle must not fake terminal_stopped"
    );
    let mouse = shellfox_ghostty_vt::MouseEvent {
        action: shellfox_ghostty_vt::MouseAction::Motion,
        button: None,
        modifiers: Default::default(),
        x: 0.0,
        y: 0.0,
        any_button_pressed: false,
    };
    let (zero, zero_context) = fixture.request_with_context(
        Command::Mouse {
            lease,
            event: mouse,
            geometry: shellfox_ghostty_vt::MouseGeometry {
                cell_width: 8,
                cell_height: 16,
            },
        },
        Some(null),
    );
    assert!(matches!(zero, Response::Input(receipt) if receipt.accepted_bytes == 0));
    assert_eq!(
        fixture.runtime().live_output_status(),
        shellfox_runtime::LiveOutputStatus::Active
    );
    assert!(
        !persistence
            .accepted_receipt(
                &host,
                zero_context.unwrap(),
                policy.clock().now_ms().unwrap()
            )
            .resumed
    );
    assert_eq!(persistence.pending_count(), 0);
    assert_eq!(host.snapshot().unwrap().sessions[0].settled_at, first);
    // Freeze the displayed marker before re-settling; dispatch must not recapture.
    let old = InputAdmission::raw(
        b"queued\r",
        true,
        true,
        shellfox_host::CapturedSettlement::Captured(first.clone()),
    )
    .unwrap();
    host.set_session_settled(identity.session_id, false)
        .unwrap();
    let resettled = host.set_session_settled(identity.session_id, true).unwrap();
    let current = resettled.sessions[0].settled_at.clone();
    assert_ne!(first, current);
    let (written, captured) = fixture.request_with_context(
        Command::RawInput {
            lease,
            bytes: b"queued\r".to_vec(),
        },
        Some(old),
    );
    assert!(matches!(written, Response::Input(receipt) if receipt.accepted_bytes == 7));
    let stale =
        persistence.accepted_receipt(&host, captured.unwrap(), policy.clock().now_ms().unwrap());
    assert!(!stale.resumed);
    assert!(stale.metadata_error.is_none());
    assert_eq!(host.snapshot().unwrap().sessions[0].settled_at, current);
    assert_eq!(persistence.pending_count(), 1);
    // Accepted bytes need not be read/executed: the controlled root now waits for
    // its child, but its input pipe still accepts these small complete writes.
    let matching = InputAdmission::raw(
        b"new\r",
        true,
        true,
        shellfox_host::CapturedSettlement::Captured(current),
    )
    .unwrap();
    let (written, captured) = fixture.request_with_context(
        Command::RawInput {
            lease,
            bytes: b"new\r".to_vec(),
        },
        Some(matching),
    );
    assert!(matches!(written, Response::Input(receipt) if receipt.accepted_bytes == 4));
    assert!(
        persistence
            .accepted_receipt(&host, captured.unwrap(), policy.clock().now_ms().unwrap())
            .resumed
    );
    assert!(host.snapshot().unwrap().sessions[0].settled_at.is_none());
    assert_eq!(persistence.pending_count(), 0);
    host.set_session_settled(identity.session_id, true).unwrap();
    let prepared = host.prepare_close_tab(identity.tab_id).unwrap();
    let legacy = InputAdmission::raw(
        b"after\r",
        true,
        true,
        shellfox_host::CapturedSettlement::Legacy,
    )
    .unwrap();
    let (written, captured) = fixture.request_with_context(
        Command::RawInput {
            lease,
            bytes: b"after\r".to_vec(),
        },
        Some(legacy),
    );
    assert!(matches!(written, Response::Input(receipt) if receipt.accepted_bytes == 6));
    let deferred =
        persistence.accepted_receipt(&host, captured.unwrap(), policy.clock().now_ms().unwrap());
    assert_eq!(
        deferred.metadata_error,
        Some(ActivityPersistenceFailure::Storage)
    );
    assert_eq!(persistence.pending_count(), 1);
    host.cancel_close(prepared.token).unwrap();
    assert!(matches!(
        persistence.flush_next(&host, policy.clock().now_ms().unwrap(), true),
        ActivityFlush::Committed { .. }
    ));
    fixture.close();
    if policy.confirmed_exit(identity).touch_session {
        persistence
            .touch(identity.session_id, policy.clock().now_ms().unwrap())
            .unwrap();
    }
    persistence.flush_next(&host, policy.clock().now_ms().unwrap(), true);
    assert_eq!(persistence.pending_count(), 0);
    host.terminal_stopped(registered).unwrap();
    drop(host);
    scratch.close().unwrap();
}
