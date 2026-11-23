use super::*;
use crate::activity::*;
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_runtime::{Identity, ProcessInspection};
use shellfox_terminal::inspection::{
    InspectionPlatform, RawOwnedEvidence, RawProcessEvidence, RawProcessIdentity,
};
use std::{collections::HashMap, ffi::OsString};

fn id() -> Identity {
    Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    }
}
fn raw(identity: Identity) -> ProcessInspection {
    let root = RawProcessIdentity {
        pid: 100,
        birth: u64::MAX - 1,
    };
    ProcessInspection {
        identity,
        evidence: RawOwnedEvidence {
            platform: InspectionPlatform::Windows,
            root,
            root_alive: true,
            complete: true,
            processes: vec![RawProcessEvidence {
                identity: root,
                is_root: true,
                accessible: true,
                executable: Some("C:\\工具\\cmd.exe".into()),
                process_name: Some("cmd.exe".into()),
                argv: Some(vec!["cmd.exe".into(), "/D".into()]),
                parent_pid: None,
                memory_bytes: None,
                started_ms: None,
            }],
        },
    }
}
struct Pending {
    calls: usize,
}
impl ProcessInspector for Pending {
    fn inspect(&mut self, _: Identity) -> Result<OwnedProcessSnapshot, InspectionFailure> {
        self.calls += 1;
        Err(InspectionFailure::Pending)
    }
}
struct Ready;
impl ProcessInspector for Ready {
    fn inspect(&mut self, identity: Identity) -> Result<OwnedProcessSnapshot, InspectionFailure> {
        convert_inspection(identity, raw(identity))
    }
}

#[test]
fn pending_is_not_new_process_evidence_or_fresh_expiry_proof() {
    let identity = id();
    let mut monitor = ActivityMonitor::new();
    monitor.watch(identity, "cmd.exe".into()).unwrap();
    let previous = monitor.poll(&mut Ready, &[], 0, 1).remove(0).observation;
    let mut pending = Pending { calls: 0 };
    assert!(monitor.poll(&mut pending, &[], 1000, 1).is_empty());
    assert_eq!(monitor.observation(identity), Some(&previous));
    assert!(!monitor.fresh_healthy(&[identity], 1000));
    assert!(monitor.poll(&mut pending, &[], 1001, 1).is_empty());
    assert_eq!(pending.calls, 2);
}

#[test]
fn pending_requests_consume_bounded_poll_budget_fairly() {
    let mut monitor = ActivityMonitor::new();
    for _ in 0..40 {
        monitor.watch(id(), "cmd.exe".into()).unwrap();
    }
    let mut pending = Pending { calls: 0 };
    assert!(monitor.poll(&mut pending, &[], 0, usize::MAX).is_empty());
    assert_eq!(pending.calls, 16);
    monitor.poll(&mut pending, &[], 0, 1);
    assert_eq!(pending.calls, 17);
}

#[test]
fn conversion_preserves_exact_birth_and_valid_unicode_without_guardian_substitution() {
    let identity = id();
    let snapshot = convert_inspection(identity, raw(identity)).unwrap();
    assert_eq!(snapshot.processes.len(), 1);
    assert!(snapshot.processes[0].is_root);
    assert_eq!(
        snapshot.processes[0].identity.birth,
        (u64::MAX - 1).to_string()
    );
    assert_eq!(
        snapshot.processes[0].executable.as_deref(),
        Some("C:\\工具\\cmd.exe")
    );
    let mut wrong = raw(identity);
    wrong.evidence.processes[0].identity.pid = 99;
    assert!(matches!(
        convert_inspection(identity, wrong),
        Err(InspectionFailure::OwnershipMismatch)
    ));
    let mut wrong = raw(identity);
    wrong.identity.generation = Generation::new();
    assert!(matches!(
        convert_inspection(identity, wrong),
        Err(InspectionFailure::OwnershipMismatch)
    ));
    let mut missing = raw(identity);
    missing.evidence.processes[0].is_root = false;
    assert!(matches!(
        convert_inspection(identity, missing),
        Err(InspectionFailure::OwnershipMismatch)
    ));
}

#[test]
fn text_conversion_and_total_bytes_are_bounded_before_publication() {
    let identity = id();
    let mut oversized = raw(identity);
    oversized.evidence.processes[0].argv = Some(vec![OsString::from("s".repeat(64 * 1024)); 129]);
    assert!(matches!(
        convert_inspection(identity, oversized),
        Err(InspectionFailure::Bounds)
    ));
    let mut oversized = raw(identity);
    oversized.evidence.processes[0].executable = Some("s".repeat(64 * 1024 + 1).into());
    assert!(matches!(
        convert_inspection(identity, oversized),
        Err(InspectionFailure::Bounds)
    ));
    let mut unix = raw(identity);
    unix.evidence.platform = InspectionPlatform::Unix;
    let snapshot = convert_inspection(identity, unix).unwrap();
    assert_eq!(snapshot.platform, crate::activity::ProcessPlatform::Unix);
    assert_eq!(snapshot.processes[0].identity.domain, "unix/local");
}

#[test]
fn lossy_os_text_remains_visible_only_as_unknown_not_healthy_matching_evidence() {
    let identity = id();
    #[cfg(windows)]
    let invalid = {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[0xd800])
    };
    #[cfg(unix)]
    let invalid = {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![255])
    };
    #[cfg(any(windows, unix))]
    {
        let mut evidence = raw(identity);
        evidence.evidence.processes[0].process_name = Some(invalid);
        let snapshot = convert_inspection(identity, evidence).unwrap();
        assert!(!snapshot.complete);
        assert!(!snapshot.processes[0].accessible);
        assert!(snapshot.processes[0]
            .process_name
            .as_ref()
            .unwrap()
            .contains('\u{fffd}'));
    }
}

#[test]
fn owner_hooks_preserve_input_intent_without_using_runtime_phase_as_agent_status() {
    let identity = id();
    let mut owner = OwnerActivity::new();
    owner
        .watch(identity, "cmd.exe".into(), InspectionScope::LocalWindows)
        .unwrap();
    let sink = owner.live_output_sink(identity).unwrap();
    sink.register_input(
        identity,
        shellfox_runtime::RequestId(1),
        InputAdmission::raw(
            b"x",
            true,
            true,
            shellfox_host::CapturedSettlement::Captured(None),
        )
        .unwrap(),
    )
    .unwrap();
    shellfox_runtime::LiveOutputSink::before_input(
        sink.as_ref(),
        identity,
        shellfox_runtime::RequestId(1),
        shellfox_runtime::InputKind::Raw,
        b"x",
        owner.clock().instant_at(0).unwrap(),
    )
    .unwrap();
    shellfox_runtime::LiveOutputSink::output(
        sink.as_ref(),
        identity,
        1,
        b"echo",
        owner.clock().instant_at(0).unwrap(),
    )
    .unwrap();
    let echoed = owner.take_live_updates().remove(0);
    assert_eq!(echoed.output_at_ms, Some(0));
    assert_eq!(echoed.busy_changed, None);
    assert!(echoed.available);
    let receipt = Ok(shellfox_runtime::Response::Input(
        shellfox_runtime::InputReceipt {
            accepted_bytes: 1,
            kind: shellfox_runtime::InputKind::Raw,
        },
    ));
    assert!(
        sink.finish_input(identity, shellfox_runtime::RequestId(1), &receipt)
            .unwrap()
            .unwrap()
            .activity
            .touch_session
    );
    shellfox_runtime::LiveOutputSink::output(
        sink.as_ref(),
        identity,
        2,
        b"work",
        owner.clock().instant_at(250).unwrap(),
    )
    .unwrap();
    assert_eq!(owner.take_live_updates()[0].busy_changed, Some(true));
    assert_eq!(owner.tick(2750), vec![(identity, false)]);
    let settled = Some("2026-01-01T00:00:00.000Z");
    assert_eq!(
        owner.accepted_input(identity, b"x", true, settled, Some(None)),
        InputActivity {
            touch_session: true,
            unsettle: false
        }
    );
    assert!(
        owner
            .accepted_input(identity, b"x", true, settled, Some(settled))
            .unsettle
    );
    assert_eq!(
        owner.accepted_input(identity, b"\x1b[I", true, settled, None),
        InputActivity::default()
    );
    assert_eq!(
        owner.accepted_input(identity, b"reply", false, settled, None),
        InputActivity::default()
    );
    assert!(owner
        .terminal_facts(identity, shellfox_runtime::Phase::Active)
        .observation
        .is_none());
    owner.forget(Identity {
        generation: Generation::new(),
        ..identity
    });
    assert!(
        owner
            .accepted_input(identity, b"x", true, settled, None)
            .touch_session
    );
    owner.forget(identity);
    assert_eq!(
        owner.accepted_input(identity, b"x", true, settled, None),
        InputActivity::default()
    );
}

#[test]
fn unsupported_scopes_and_missing_runtime_do_not_request_or_fake_processes() {
    let identity = id();
    let source = HashMap::<TabId, shellfox_runtime::Runtime>::new();
    let mut state = RuntimeInspectionState::new();
    for scope in [
        InspectionScope::Guest,
        InspectionScope::Ssh,
        InspectionScope::Unsupported,
    ] {
        state.watch(identity, scope).unwrap();
        let mut inspector = RuntimeProcessInspector::new(&source, &mut state, 0);
        assert!(matches!(
            inspector.inspect(identity),
            Err(InspectionFailure::Unavailable)
        ));
    }
    state
        .watch(identity, InspectionScope::LocalWindows)
        .unwrap();
    let mut inspector = RuntimeProcessInspector::new(&source, &mut state, 0);
    assert!(matches!(
        inspector.inspect(identity),
        Err(InspectionFailure::OwnershipMismatch)
    ));
    assert!(!state.fresh_since(&[identity], 0));
}
