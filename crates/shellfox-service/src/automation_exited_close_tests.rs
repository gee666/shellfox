use super::*;
use crate::automation::{AutomationRetirementAck, AutomationViewer};
use anyhow::{ensure, Result};
use shellfox_protocol::automation::{AutomationRun, AutomationShow, AutomationState};
struct Viewer(Identity);
impl AutomationViewer for Viewer {
    fn create_read_only(&mut self, _: &AutomationRun) -> Result<(Uuid, Uuid)> {
        Ok((
            self.0.session_id.to_string().parse()?,
            self.0.tab_id.to_string().parse()?,
        ))
    }
    fn focus(&mut self, session: Uuid) -> Result<()> {
        ensure!(
            session.to_string() == self.0.session_id.to_string(),
            "Unexpected test view identity"
        );
        Ok(())
    }
}
fn identity() -> Identity {
    Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    }
}
fn launch(
    manager: &mut AutomationRuns,
    helper: &std::path::Path,
    directory: &std::path::Path,
    mode: &str,
) -> (AutomationRun, Runtime, AutomationRelay) {
    let input = AutomationStart {
        request_id: Uuid::new_v4(),
        executable: helper.to_str().unwrap().into(),
        args: vec![mode.into()],
        cwd: directory.to_str().unwrap().into(),
        env: BTreeMap::new(),
        title: None,
        detached: false,
    };
    manager.reserve_start(&input, 1).unwrap();
    let source = manager.take_source(input.request_id).unwrap();
    let identity = identity();
    let mut runtime = Runtime::start_external_with_policy(
        identity,
        ExternalConfig::default(),
        ExternalDisplayPolicy::LegacyAutomationV1,
    )
    .unwrap();
    let relay = AutomationRelay::new(source, &mut runtime).unwrap();
    manager
        .bind_source_identity(input.request_id, identity)
        .unwrap();
    (manager.start(input, 1).unwrap(), runtime, relay)
}
fn joined(runtime: &mut Runtime, deadline: Instant) {
    runtime.request_external_close().unwrap();
    while !runtime.poll_external_closed().unwrap() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
#[ignore = "Explicit owned Windows raw fixture. Set SHELLFOX_RAW_TEST_HELPER and run --ignored."]
fn explicit_close_releases_only_exited_runs_unacked_credit_without_fabricating_ack() {
    let helper = PathBuf::from(
        std::env::var_os("SHELLFOX_RAW_TEST_HELPER").expect("Explicit owned helper required"),
    );
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tmp")
        .join(format!("automation-exited-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let scratch = Scratch(directory);
    let mut manager = AutomationRuns::new(helper.clone());
    let (run, mut runtime, mut relay) = launch(&mut manager, &helper, &scratch.0, "describe");
    let (other, mut other_runtime, mut other_relay) =
        launch(&mut manager, &helper, &scratch.0, "idle");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !manager.is_stopped(run.id).unwrap() || !relay.finished(&runtime) {
        manager.pump().unwrap();
        relay.pump().unwrap();
        other_relay.pump().unwrap();
        while runtime.try_recv().is_ok() {}
        while other_runtime.try_recv().is_ok() {}
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(manager.runs[&run.id].dto.state, AutomationState::Exited);
    assert!(!manager.runs[&run.id].frames.is_empty());
    assert_eq!(manager.runs[&run.id].acknowledged, 0);
    let identity = runtime.identity();
    manager
        .show(AutomationShow { id: run.id }, &mut Viewer(identity))
        .unwrap();
    assert!(manager.begin_retire(run.id).is_err());
    assert!(manager.runs.contains_key(&run.id));
    manager.begin_stop(AutomationStop { id: run.id }).unwrap();
    assert!(manager.runs[&run.id].frames.is_empty());
    assert_eq!(manager.runs[&run.id].acknowledged, 0);
    assert!(manager.runs[&other.id].capturing);
    assert!(!manager.runs[&other.id].input_ended);
    assert!(relay.finished(&runtime));
    assert!(!runtime.external_status().unwrap().closed);
    let receipt = manager.begin_retire(run.id).unwrap();
    assert!(receipt.publication().is_some());
    let proof = receipt.proof();
    joined(&mut runtime, deadline);
    manager
        .ack_retired(
            proof,
            AutomationRetirementAck::PublishedParserClosed { identity },
        )
        .unwrap();
    assert!(proof.raw_cleanup_confirmed());
    assert_eq!(proof.runtime_identity(), identity);
    assert!(manager.is_stopped(run.id).is_err());
    manager.begin_stop(AutomationStop { id: other.id }).unwrap();
    while !manager.is_stopped(other.id).unwrap() || !other_relay.finished(&other_runtime) {
        manager.pump().unwrap();
        other_relay.pump().unwrap();
        while other_runtime.try_recv().is_ok() {}
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(manager.runs[&other.id].dto.exit_code.is_none());
    assert_eq!(
        manager.runs[&other.id].dto.signal.as_deref(),
        Some("SIGKILL")
    );
    let proof = manager.begin_retire(other.id).unwrap().proof();
    assert!(proof.kernel_exit_code() > 255);
    let identity = other_runtime.identity();
    joined(&mut other_runtime, deadline);
    manager
        .ack_retired(
            proof,
            AutomationRetirementAck::UnshownParserClosed { identity },
        )
        .unwrap();
    manager.shutdown().unwrap();
}
