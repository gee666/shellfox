use super::*;
use shellfox_core::{Generation, SessionId, TabId};
use shellfox_runtime::{ExternalConfig, ExternalDisplayPolicy, Identity, LaunchPrivilege, Runtime};
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(6);
        while std::fs::remove_dir_all(&self.0).is_err() {
            if Instant::now() >= deadline {
                assert!(
                    std::thread::panicking(),
                    "Owned launch fixture cleanup failed"
                );
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
struct Launched {
    manager: AutomationRuns,
    runtime: Runtime,
    relay: AutomationRelay,
    id: Uuid,
    _scratch: Scratch,
}
impl Launched {
    fn new(bound: bool) -> Self {
        let helper = PathBuf::from(
            std::env::var_os("SHELLFOX_RAW_TEST_HELPER").expect("Explicit owned helper required"),
        );
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tmp")
            .join(format!("automation-launch-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let scratch = Scratch(directory);
        let input = AutomationStart {
            request_id: Uuid::new_v4(),
            executable: helper.to_str().unwrap().into(),
            args: vec!["exit-code".into(), "7".into()],
            cwd: scratch.0.to_str().unwrap().into(),
            env: BTreeMap::new(),
            title: None,
            detached: false,
        };
        let id = input.request_id;
        let mut manager = AutomationRuns::new(helper);
        manager.reserve_start(&input, 1).unwrap();
        let source = manager.take_source(id).unwrap();
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
        manager.bind_source_identity(id, identity).unwrap();
        if bound {
            let binding = relay
                .take_launch_binding()
                .expect("One private factory binding");
            assert!(relay.take_launch_binding().is_none());
            manager
                .start_with_launch_binding(input, 1, binding)
                .unwrap();
        } else {
            manager.start(input, 1).unwrap();
        }
        Self {
            manager,
            runtime,
            relay,
            id,
            _scratch: scratch,
        }
    }
    fn finish(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.manager.is_stopped(self.id).unwrap() || !self.relay.finished(&self.runtime) {
            self.manager.pump().unwrap();
            self.relay.pump().unwrap();
            while self.runtime.try_recv().is_ok() {}
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn join(&mut self) {
        self.runtime.request_external_close().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.runtime.poll_external_closed().unwrap() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
struct Viewer(Identity);
impl AutomationViewer for Viewer {
    fn create_read_only(&mut self, _: &AutomationRun) -> Result<(Uuid, Uuid)> {
        Ok((
            self.0.session_id.to_string().parse()?,
            self.0.tab_id.to_string().parse()?,
        ))
    }
    fn focus(&mut self, id: Uuid) -> Result<()> {
        ensure!(
            id.to_string() == self.0.session_id.to_string(),
            "Wrong canonical view"
        );
        Ok(())
    }
}
// The receipt carries the Windows primary token.
#[cfg(windows)]
#[test]
#[ignore = "Explicit owned Windows raw fixture. Set SHELLFOX_RAW_TEST_HELPER and run --ignored."]
fn receipt_survives_natural_cleanup_and_published_parser_retirement_without_new_handles() {
    let mut run = Launched::new(true);
    run.finish();
    assert!(run.manager.runs[&run.id].process.is_none());
    assert!(
        run.manager.begin_retire(run.id).is_err(),
        "Untaken native receipt must not be silently evicted"
    );
    assert_eq!(
        run.runtime.launch_privilege_facts().privilege,
        LaunchPrivilege::Unknown
    );
    let evidence = run
        .manager
        .take_launch_evidence(run.id)
        .unwrap()
        .expect("Natural cleanup must not discard snapshot");
    assert!(run.manager.take_launch_evidence(run.id).unwrap().is_none());
    let facts = evidence.facts();
    assert!(facts.token.is_some());
    run.relay.bind_launch_evidence(evidence).unwrap();
    assert_eq!(run.runtime.launch_privilege_facts(), facts);
    let identity = run.runtime.identity();
    run.manager
        .show(AutomationShow { id: run.id }, &mut Viewer(identity))
        .unwrap();
    run.manager
        .begin_stop(AutomationStop { id: run.id })
        .unwrap();
    let receipt = run.manager.begin_retire(run.id).unwrap();
    run.manager
        .ack_retired(
            receipt.proof(),
            AutomationRetirementAck::PublishedParserTransferred { identity },
        )
        .unwrap();
    assert_eq!(run.runtime.launch_privilege_facts(), facts);
    assert!(!run.runtime.external_status().unwrap().closed);
    run.join();
    assert_eq!(run.runtime.launch_privilege_facts(), facts);
}
#[test]
#[ignore = "Explicit owned raw fixture. Set SHELLFOX_RAW_TEST_HELPER and run --ignored."]
fn wrong_runtime_receipt_rejected_and_unbound_start_stays_unknown() {
    let mut captured = Launched::new(true);
    let mut other = Launched::new(false);
    captured.finish();
    other.finish();
    assert!(other
        .manager
        .take_launch_evidence(other.id)
        .unwrap()
        .is_none());
    let evidence = captured
        .manager
        .take_launch_evidence(captured.id)
        .unwrap()
        .unwrap();
    assert!(other.relay.bind_launch_evidence(evidence).is_err());
    assert_eq!(
        other.runtime.launch_privilege_facts().privilege,
        LaunchPrivilege::Unknown
    );
    assert_eq!(
        captured.runtime.launch_privilege_facts().privilege,
        LaunchPrivilege::Unknown
    );
    captured.join();
    other.join();
}
