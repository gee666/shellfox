use shellfox_core::{
    CreateSession, Generation, ProfileId, SessionId, SettingsAdapter, SettingsShell, TabId,
};
use shellfox_host::{CapturedSettlement, Host, SessionCreationFacts};
use shellfox_runtime::{
    Identity, InputKind, InputReceipt, LiveOutputSink, Phase, RequestId, Response, Runtime,
};
use shellfox_service::{activity::*, activity_integration::*};
use std::{collections::HashMap, path::PathBuf};

#[test]
fn ssh_program_keeps_live_activity_and_input_context_without_inspection_or_creation_relabelling() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root).unwrap();
    let scratch = tempfile::Builder::new()
        .prefix("activity-program-")
        .tempdir_in(root)
        .unwrap();
    let host = Host::open(scratch.path()).unwrap();
    let input = |profile| CreateSession {
        request_id: SessionId::new(),
        title: None,
        cwd: scratch.path().to_string_lossy().into_owned(),
        profile_id: ProfileId::new(profile).unwrap(),
    };
    let historical = host
        .create_session_with_facts(
            input("cmd"),
            SessionCreationFacts::new(SettingsAdapter::EmbeddedPty, SettingsShell::LoginShell),
        )
        .unwrap();
    let identity = Identity {
        session_id: historical.selection.session_id.unwrap(),
        tab_id: historical.selection.tab_id.unwrap(),
        generation: Generation::new(),
    };
    let genuine = host.session_creation_facts().unwrap();
    // An SSH-only arbitrary program intent is fact-less AT CREATION, not a fake
    // Cmd/Pwsh session that gets hidden or relabelled while capturing a snapshot.
    let program = host
        .create_session(input("opaque:ssh-program"))
        .unwrap()
        .selection
        .session_id
        .unwrap();
    assert!(!host
        .session_creation_facts()
        .unwrap()
        .contains_key(&program));
    let mut policy = OwnerActivity::new();
    policy
        .watch(identity, "C:\\Tools\\ssh.exe".into(), InspectionScope::Ssh)
        .unwrap();
    let runtimes = HashMap::<TabId, Runtime>::new(); // No Runtime, PTY or process is created.
    let inspected = policy.poll_processes(&runtimes, &[], 0, 1, true);
    assert_eq!(inspected.len(), 1);
    assert_eq!(inspected[0].observation.root, RootState::Unavailable);
    assert_eq!(inspected[0].observation.health, Health::Unknown);
    assert_eq!(inspected[0].observation.agents, 0);
    let sink = policy.live_output_sink(identity).unwrap();
    sink.output(
        identity,
        1,
        b"remote output",
        policy.clock().instant_at(100).unwrap(),
    )
    .unwrap();
    let work = policy.take_live_updates().remove(0);
    assert_eq!(work.output_at_ms, Some(100));
    assert_eq!(work.busy_changed, Some(true));
    let mut ttl = SessionActivity::new();
    ttl.touch(work.output_at_ms.unwrap());
    assert_eq!(
        ttl.last_action_ms(SessionAging {
            updated_ms: 0,
            settled_ms: None,
            pinned: false
        }),
        100
    );
    sink.register_input(
        identity,
        RequestId(1),
        InputAdmission::raw(b"x", true, true, CapturedSettlement::Captured(None)).unwrap(),
    )
    .unwrap();
    sink.before_input(
        identity,
        RequestId(1),
        InputKind::Raw,
        b"x",
        policy.clock().instant_at(200).unwrap(),
    )
    .unwrap();
    sink.output(
        identity,
        2,
        b"echo",
        policy.clock().instant_at(200).unwrap(),
    )
    .unwrap();
    let echo = policy.take_live_updates().remove(0);
    assert_eq!(echo.output_at_ms, Some(200));
    assert_eq!(echo.meaningful_at_ms, None);
    let reply = Ok(Response::Input(InputReceipt {
        accepted_bytes: 1,
        kind: InputKind::Raw,
    }));
    let accepted = sink
        .finish_input(identity, RequestId(1), &reply)
        .unwrap()
        .unwrap();
    assert!(accepted.activity.touch_session);
    assert_eq!(
        accepted.admission.captured_settlement,
        CapturedSettlement::Captured(None)
    );
    let (mut publisher, watch) = SourceFactsPublisher::new();
    publisher
        .publish_with_creation_facts(
            host.snapshot().unwrap(),
            host.global_settings().unwrap(),
            vec![policy.terminal_facts(identity, Phase::Active)],
            host.session_creation_facts().unwrap(),
            shellfox_protocol::CancellationToken::new(),
        )
        .unwrap();
    let published = read_source(&watch).unwrap();
    assert_eq!(published.creation_facts, genuine);
    assert!(!published.creation_facts.contains_key(&program));
    assert!(published.terminals[0].activity_available);
    assert!(
        published.terminals[0].observation.is_none(),
        "SSH remote processes remain uninspected"
    );
    drop(host);
    scratch.close().unwrap();
}
