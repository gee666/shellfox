#![cfg(windows)]
mod activity_tests;
mod close_tests;
mod facts_tests;
mod failure;
mod attach_tests;
use super::*;
use crate::{
    desktop_launch::*,
    native::{NativeAction, NativeResponse},
};
use std::collections::BTreeMap;

struct Owned(Service);
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.0.shutdown();
    }
}
fn service() -> (tempfile::TempDir, Owned, PathBuf) {
    let scratch = crate::tests::scratch();
    let helper = PathBuf::from(
        std::env::var_os("SHELLFOX_RAW_TEST_HELPER")
            .expect("explicit owned raw fixture executable"),
    );
    assert!(helper.is_absolute() && helper.is_file());
    let service = Service::open_owned(scratch.path(), &helper).unwrap();
    (scratch, Owned(service), helper)
}
fn input(helper: &std::path::Path, cwd: &std::path::Path, args: Vec<String>) -> AutomationStart {
    AutomationStart {
        request_id: Uuid::new_v4(),
        executable: helper.to_str().unwrap().into(),
        args,
        cwd: cwd.to_str().unwrap().into(),
        env: BTreeMap::from([("FIXTURE_ONLY".into(), "private fixture value".into())]),
        title: Some("Owned automation fixture".into()),
        detached: true,
    }
}
fn finish(service: &mut Service, id: Uuid) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        service.pump();
        if service.automation.runs.is_stopped(id).unwrap()
            && service.automation.relays.get(&id).is_some_and(|relay| {
                service
                    .terminals
                    .get(&relay.feed.identity().tab_id)
                    .is_some_and(|terminal| relay.feed.finished(&terminal.runtime))
            })
        {
            return;
        }
        assert!(Instant::now() < deadline, "owned source did not finish");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn show(service: &mut Service, id: Uuid) -> AutomationRun {
    let mut router = DesktopLaunchRouter::new();
    let _registration = router.register(router.owner(), service).unwrap();
    service
        .show_automation(AutomationShow { id }, &mut router)
        .unwrap()
}
fn observe(service: &mut Service, tab: TabId) {
    assert!(matches!(
        service
            .execute_native(NativeAction::Observe { tab }, 77)
            .unwrap(),
        NativeResponse::Observed(_)
    ));
}
#[test]
#[ignore = "explicit owned raw fixture only; set SHELLFOX_RAW_TEST_HELPER"]
fn late_show_uses_reserved_canonical_identity_without_facts_selection_or_control() {
    let (scratch, mut owned, helper) = service();
    let service = &mut owned.0;
    let mut spec = input(
        &helper,
        scratch.path(),
        vec![
            "describe".into(),
            "a b".into(),
            "".into(),
            "$HOME".into(),
            "%PATH%".into(),
            "\"quoted\"".into(),
        ],
    );
    spec.detached = false;
    let run = service.start_automation(spec.clone(), 9).unwrap();
    assert_eq!(
        service.start_automation(spec.clone(), 9).unwrap().pid,
        run.pid
    );
    let identity = service.automation.relays[&run.id].feed.identity();
    let mut conflict = spec;
    conflict.args.push("different".into());
    assert!(service.start_automation(conflict, 9).is_err());
    finish(service, run.id);
    let output = service
        .automation
        .runs
        .read(
            AutomationRead {
                id: run.id,
                after: 0,
            },
            9,
        )
        .unwrap();
    let mut described = Vec::new();
    let mut after = 0;
    for frame in output.frames {
        after = frame.sequence;
        assert!(matches!(frame.stream, AutomationStream::Stdout));
        described.extend(shellfox_protocol::decode_chunk(&frame.data).unwrap());
        assert!(described.len() <= 65536);
    }
    service
        .automation
        .runs
        .read(AutomationRead { id: run.id, after }, 9)
        .unwrap();
    let described: serde_json::Value = serde_json::from_slice(&described).unwrap();
    assert!(
        described["args"] == serde_json::json!(["a b", "", "$HOME", "%PATH%", "\"quoted\""]),
        "Literal argv changed."
    );
    assert!(
        described["env"] == serde_json::json!({"FIXTURE_ONLY":"private fixture value"}),
        "Explicit environment changed."
    );
    for field in ["stdinTty", "stdoutTty", "stderrTty"] {
        assert_eq!(described[field], false);
    }
    let cwd = PathBuf::from(described["cwd"].as_str().unwrap());
    assert_eq!(
        cwd.canonicalize().unwrap(),
        scratch.path().canonicalize().unwrap()
    );
    assert!(service.host.snapshot().unwrap().sessions.is_empty());
    let before = service.host.snapshot().unwrap().selection;
    let shown = show(service, run.id);
    let tab: TabId = shown.tab_id.unwrap().to_string().parse().unwrap();
    assert_eq!(tab, identity.tab_id);
    assert_eq!(service.terminals[&tab].runtime.identity(), identity);
    assert_eq!(service.terminals.len(), 1);
    let snapshot = service.host.snapshot().unwrap();
    assert_eq!(snapshot.selection, before);
    assert_eq!(
        snapshot.sessions[0].tabs[0].profile_id.as_str(),
        "automation-pipe"
    );
    assert!(!service
        .host
        .session_creation_facts()
        .unwrap()
        .contains_key(&identity.session_id));
    assert!(service.attach(tab, 20, false).is_err());
    observe(service, tab);
    let again = show(service, run.id);
    assert_eq!(again.session_id, shown.session_id);
    assert_eq!(service.host.snapshot().unwrap().sessions.len(), 1);
    let born = service.host.snapshot().unwrap().sessions[0]
        .updated_at
        .clone();
    std::thread::sleep(Duration::from_millis(15));
    service.activity.next_poll_ms = 0;
    service.pump();
    service.flush_activity(true).unwrap();
    assert_eq!(
        service.host.snapshot().unwrap().sessions[0].updated_at,
        born,
        "late Show replayed historical output/exit activity"
    );
    service.close_tab(tab).unwrap();
    assert!(!service.terminals.contains_key(&tab));
    assert!(matches!(
        service
            .host
            .reserved_publication_receipt(run.id.to_string().parse().unwrap())
            .unwrap(),
        shellfox_host::ReservedPublicationReceipt::TargetDeleted { .. }
    ));
    assert_eq!(
        service.shutdown().unwrap().presentation,
        PresentationShutdownState::NoKnownFailure
    );
}
#[test]
#[ignore = "explicit owned raw fixture only; set SHELLFOX_RAW_TEST_HELPER"]
fn published_retirement_transfers_proof_keeps_late_observer_and_blocks_id_reuse() {
    let (scratch, mut owned, helper) = service();
    let service = &mut owned.0;
    const MARKER: &str = "TB01_COMPLETED_REPLAY";
    let spec = input(&helper, scratch.path(), vec!["describe".into(), MARKER.into()]);
    let run = service.start_automation(spec.clone(), 9).unwrap();
    finish(service, run.id);
    let shown = show(service, run.id);
    let tab: TabId = shown.tab_id.unwrap().to_string().parse().unwrap();
    let identity = service.terminals[&tab].runtime.identity();
    assert_eq!(
        service.completed_automation_exit_code(&service.terminals[&tab]),
        Some(0)
    );
    let launch=service.terminals[&tab].runtime.launch_privilege_facts();
    assert_eq!(launch.purpose,shellfox_runtime::LaunchFactoryPurpose::StandardRawCreateProcess);
    assert_eq!(launch.basis,shellfox_runtime::LaunchEvidenceBasis::ReviewedSupervisorPrimaryTokenInheritance);
    assert_eq!(launch.stage,shellfox_runtime::LaunchEvidenceStage::BeforeRawGoConfirmedReady);
    service.automation.runs.begin_retire(run.id).unwrap();
    service.pump_automation_retirement().unwrap();
    assert!(!service.automation.relays.contains_key(&run.id));
    assert_eq!(service.terminals[&tab].runtime.identity(), identity);
    assert_eq!(service.terminals[&tab].runtime.launch_privilege_facts(),launch);
    assert!(service.terminals[&tab].external_cleanup.is_some());
    assert_eq!(
        service.completed_automation_exit_code(&service.terminals[&tab]),
        Some(0)
    );
    assert!(service.automation.runs.retired().is_empty());
    let normal = launch.privilege == shellfox_runtime::LaunchPrivilege::Normal;
    assert_eq!(service.remote_automation_exit_code(&service.terminals[&tab]), normal.then_some(0));
    use crate::grant_requests::GrantFailure;
    use shellfox_runtime::{AnsiRecoveryCapability, Command, ErrorCode, Response};
    let viewer = service.allocate_viewer_id().unwrap();
    let offer = service.observer_grant(tab, viewer).unwrap();
    let Response::Recovery(recovery) = service.call(tab, Command::ObserverRecoverStamped {
        observer: offer.lease(),
        capability: AnsiRecoveryCapability::FormattedActiveBufferV1,
    }).unwrap() else {
        panic!("missing recovery")
    };
    assert!(String::from_utf8_lossy(&recovery.bytes).contains(MARKER));
    for takeover in [false, true] {
        let error = service.attach_manager(tab, 71, takeover).err().unwrap();
        assert!(matches!(error.downcast_ref::<GrantFailure>(), Some(GrantFailure::Rejected(error)) if error.code == ErrorCode::Unsupported));
    }
    let ungranted = terminals::Attachment {
        controller: 71,
        epoch: 1,
        generation: identity.generation,
    };
    for result in [
        service.input(tab, &ungranted, b"not accepted".to_vec()),
        service.resize(tab, &ungranted, 100, 42),
    ] {
        assert_eq!(result.unwrap_err().to_string(), "Terminal control lease is detached or stale.");
    }
    drop(offer);
    assert!(service.start_automation(spec, 9).is_err());
    service.close_tab(tab).unwrap();
    assert!(!service.terminals.contains_key(&tab));
    assert_eq!(
        service.shutdown().unwrap().presentation,
        PresentationShutdownState::NoKnownFailure
    );
}
