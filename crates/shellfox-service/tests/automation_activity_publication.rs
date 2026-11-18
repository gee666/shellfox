pub use shellfox_service::{activity, activity_integration};
#[path = "../src/owner_automation_activity.rs"]
mod gate;
#[path = "support/automation_activity_runtime.rs"]
mod runtime_tests;
use activity_integration::{ActivityClock, InspectionScope, OwnerActivity};
use gate::{watch_program, AutomationActivity};
use shellfox_core::{Generation, ProfileId, SessionId, TabId};
use shellfox_host::{Host, ReservedSessionPublication};
use shellfox_runtime::{Identity, LiveOutputSink};
use std::{
    ffi::OsStr,
    path::PathBuf,
    sync::{Arc, Barrier},
    time::Instant,
};

fn identity() -> Identity {
    Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    }
}
fn owner() -> OwnerActivity {
    OwnerActivity::with_clock(Arc::new(ActivityClock::new(Instant::now(), 0)))
}
fn host() -> (tempfile::TempDir, Host) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root).unwrap();
    let data = tempfile::Builder::new()
        .prefix("automation-activity-")
        .tempdir_in(root)
        .unwrap();
    let host = Host::open(data.path()).unwrap();
    (data, host)
}
fn publication(identity: Identity, cwd: &std::path::Path) -> ReservedSessionPublication {
    ReservedSessionPublication {
        request_id: SessionId::new(),
        session_id: identity.session_id,
        tab_id: identity.tab_id,
        title: Some("Raw output".into()),
        cwd: cwd.to_string_lossy().into_owned(),
        profile_id: ProfileId::new("automation-pipe").unwrap(),
    }
}
#[test]
fn publication_gate_accounts_racing_and_later_live_output_without_history_or_missing_metadata() {
    let identity = identity();
    let gate = Arc::new(AutomationActivity::new(identity));
    let mut owner = owner();
    let (data, host) = host();
    let hooks = gate.hooks();
    hooks
        .live_output
        .as_ref()
        .unwrap()
        .output(
            identity,
            1,
            b"before Show",
            owner.clock().instant_at(10).unwrap(),
        )
        .unwrap();
    assert!(owner.take_live_updates().is_empty());
    assert!(host.snapshot().unwrap().sessions.is_empty());
    let barrier = Arc::new(Barrier::new(2));
    let mut worker = None;
    let at = owner.clock().instant_at(20).unwrap();
    gate.publish(&mut owner, OsStr::new("arbitrary-program"), || {
        let gate = gate.clone();
        let started = barrier.clone();
        worker = Some(std::thread::spawn(move || {
            started.wait();
            gate.output(identity, 2, b"racing output", at).unwrap();
        }));
        barrier.wait();
        host.publish_reserved_session(publication(identity, data.path()), None)
            .map_err(Into::into)
    })
    .unwrap();
    worker.take().unwrap().join().unwrap();
    let actual = owner.live_output_sink(identity).unwrap();
    let update = actual.take_update().unwrap();
    assert_eq!(update.last_sequence, 2);
    assert_eq!(update.output_at_ms, Some(20));
    assert!(host.session_creation_facts().unwrap().is_empty());
    gate.output(
        identity,
        3,
        b"\x1b[?25l",
        owner.clock().instant_at(1000).unwrap(),
    )
    .unwrap();
    assert_eq!(
        actual.take_update().unwrap().output_at_ms,
        Some(1000),
        "idle escapes still refresh TTL"
    );
    assert!(actual.tick(4000).unwrap().is_some());
    assert!(
        !actual.status().0,
        "silence clears busy without an activity touch"
    );
    gate.raw_exit_observed(owner.clock().instant_at(4100).unwrap())
        .unwrap();
    assert_eq!(
        gate.take_exit_touch().and_then(|at| owner.clock().at(at)),
        Some(4100)
    );
    gate.raw_exit_observed(owner.clock().instant_at(4200).unwrap())
        .unwrap();
    assert!(gate.take_exit_touch().is_none());
}
#[test]
fn failed_publication_unbinds_new_watch_and_late_show_does_not_replay_exit_or_output() {
    let identity = identity();
    let gate = Arc::new(AutomationActivity::new(identity));
    let mut owner = owner();
    let (data, host) = host();
    let mut invalid = publication(identity, data.path());
    invalid.title = Some(String::new());
    let failure = gate.publish(&mut owner, OsStr::new("raw-program"), || {
        host.publish_reserved_session(invalid, None)
            .map_err(Into::into)
    });
    assert!(failure.is_err());
    assert!(owner.live_output_sink(identity).is_none());
    gate.output(
        identity,
        1,
        b"hidden history",
        owner.clock().instant_at(10).unwrap(),
    )
    .unwrap();
    gate.raw_exit_observed(owner.clock().instant_at(20).unwrap())
        .unwrap();
    assert!(gate.take_exit_touch().is_none());
    assert!(owner.take_live_updates().is_empty());
    assert!(host.snapshot().unwrap().sessions.is_empty());
    gate.publish(&mut owner, OsStr::new("raw-program"), || {
        host.publish_reserved_session(publication(identity, data.path()), None)
            .map_err(Into::into)
    })
    .unwrap();
    assert!(owner.take_live_updates().is_empty());
    assert!(gate.take_exit_touch().is_none());
    assert_eq!(
        owner.live_output_sink(identity).unwrap().status(),
        (false, true)
    );
    assert!(host.session_creation_facts().unwrap().is_empty());
    assert!(
        !owner.confirmed_exit(identity).touch_session,
        "late publication seeds owner exit state without a historical TTL touch"
    );
}
#[test]
fn a_later_focus_queue_failure_cannot_rollback_successfully_published_activity() {
    let identity = identity();
    let gate = Arc::new(AutomationActivity::new(identity));
    let mut owner = owner();
    let (data, host) = host();
    let shown: anyhow::Result<()> = (|| {
        gate.publish(&mut owner, OsStr::new("raw-program"), || {
            host.publish_reserved_session(publication(identity, data.path()), None)
                .map_err(Into::into)
        })?;
        anyhow::bail!("injected later focus queue failure")
    })();
    assert!(shown.is_err());
    assert_eq!(host.snapshot().unwrap().sessions.len(), 1);
    gate.output(
        identity,
        1,
        b"after failed focus",
        owner.clock().instant_at(50).unwrap(),
    )
    .unwrap();
    assert_eq!(
        owner
            .live_output_sink(identity)
            .unwrap()
            .take_update()
            .unwrap()
            .output_at_ms,
        Some(50)
    );
}
#[test]
fn program_watch_refinement_preserves_the_book_and_unsupported_inspection_does_not_disable_output()
{
    let identity = identity();
    let mut owner = owner();
    watch_program(
        &mut owner,
        identity,
        OsStr::new("actual-program"),
        InspectionScope::Unsupported,
    )
    .unwrap();
    let first = owner.live_output_sink(identity).unwrap();
    watch_program(
        &mut owner,
        identity,
        OsStr::new("actual-program"),
        InspectionScope::Ssh,
    )
    .unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &owner.live_output_sink(identity).unwrap()
    ));
    first
        .output(
            identity,
            1,
            b"still active",
            owner.clock().instant_at(10).unwrap(),
        )
        .unwrap();
    assert_eq!(first.take_update().unwrap().output_at_ms, Some(10));
    assert!(owner
        .terminal_facts(identity, shellfox_runtime::Phase::Active)
        .observation
        .is_none());
    watch_program(
        &mut owner,
        identity,
        OsStr::new(&"x".repeat(65537)),
        InspectionScope::LocalWindows,
    )
    .unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &owner.live_output_sink(identity).unwrap()
    ));
    assert!(!owner.fresh_healthy(&[identity], 0));
}
