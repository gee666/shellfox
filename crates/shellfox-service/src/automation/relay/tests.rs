use super::super::{AutomationRuns, AUTOMATION_CREDIT};
use super::*;
mod exited_close;
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_protocol::automation::{AutomationStart, AutomationStop};
use shellfox_runtime::{ExternalConfig, ExternalDisplayPolicy};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};
use uuid::Uuid;
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(6);
        while std::fs::remove_dir_all(&self.0).is_err() {
            if Instant::now() >= deadline {
                assert!(
                    std::thread::panicking(),
                    "Owned failure fixture cleanup failed"
                );
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
#[test]
#[ignore = "Explicit owned Windows raw fixture. Set SHELLFOX_RAW_TEST_HELPER and run --ignored."]
fn parser_closes_under_raw_backpressure_but_failed_cleanup_still_proves_job_tree_exit() {
    let helper = PathBuf::from(
        std::env::var_os("SHELLFOX_RAW_TEST_HELPER").expect("Explicit owned helper required"),
    );
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tmp")
        .join(format!("automation-failed-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let scratch = Scratch(directory);
    let marker = scratch.0.join("descendant-survived");
    let mut manager = AutomationRuns::new(helper.clone());
    let input = AutomationStart {
        request_id: Uuid::new_v4(),
        executable: helper.to_str().unwrap().into(),
        args: vec!["flood-branch".into(), marker.to_str().unwrap().into()],
        cwd: scratch.0.to_str().unwrap().into(),
        env: BTreeMap::new(),
        title: None,
        detached: false,
    };
    manager.reserve_start(&input, 1).unwrap();
    let source = manager.take_source(input.request_id).unwrap();
    let identity = Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    };
    let mut runtime = Runtime::start_external_with_policy(
        identity,
        ExternalConfig::default(),
        ExternalDisplayPolicy::LegacyAutomationV1,
    )
    .unwrap();
    let mut relay = AutomationRelay::new(source, &mut runtime).unwrap();
    manager
        .bind_source_identity(input.request_id, identity)
        .unwrap();
    let run = manager.start(input, 1).unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    while manager.runs[&run.id].bytes < AUTOMATION_CREDIT {
        manager.pump().unwrap();
        relay.pump().unwrap();
        while runtime.try_recv().is_ok() {}
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    // Lose the actual producer channel, not the UI observer. This causes a real
    // unfinished-source failure without weakening the Runtime's Close guard.
    relay.disconnect_for_test();
    while !runtime.external_status().unwrap().failed {
        while runtime.try_recv().is_ok() {}
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    runtime.request_external_close().unwrap();
    while !runtime.poll_external_closed().unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    manager.begin_stop(AutomationStop { id: run.id }).unwrap();
    manager.pump().unwrap();
    assert!(relay.pump().is_err());
    let failure = relay.failure().expect("Typed source rejection evidence");
    assert!(failure.pending_bytes() > 0);
    assert_eq!(failure.identity(), Some(identity));
    let (retained, original_allocation) = match relay.pending.as_ref().unwrap() {
        AutomationViewEvent::Data { bytes, .. } => (bytes.clone(), bytes.as_ptr()),
        _ => panic!("Exact failed data chunk required"),
    };
    manager
        .begin_failed_source_cleanup(run.id, failure)
        .unwrap();
    while manager.failed_source_cleanup(run.id).is_none() {
        manager.pump().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let proof = manager.failed_source_cleanup(run.id).unwrap();
    assert!(proof.raw_cleanup_confirmed());
    assert_eq!(proof.run_id(), run.id);
    assert!(proof.failure() == failure);
    assert!(manager.is_stopped(run.id).unwrap());
    assert_eq!(manager.active_processes(), 0);
    assert!(!relay.finished(&runtime));
    assert!(!runtime.external_status().unwrap().finished);
    assert_ne!(
        manager.runs[&run.id].eof, [true; 2],
        "Failed teardown must not fake both EOFs"
    );
    assert!(
        matches!(relay.pending.as_ref(),Some(AutomationViewEvent::Data {bytes,..}) if *bytes==retained)
    );
    let evidence = relay.take_failed_evidence(proof, &mut runtime).unwrap();
    assert_eq!(evidence.pending_for_test().unwrap(), retained);
    assert_eq!(
        evidence.pending_for_test().unwrap().as_ptr(),
        original_allocation
    );
    assert!(relay.take_failed_evidence(proof, &mut runtime).is_err());
    assert!(evidence.proof().raw_cleanup_confirmed());
    assert!(!marker.exists());
    manager.shutdown().unwrap();
    assert!(manager.shutdown_ready());
}
