//! Actual sealed failed-source evidence, not a fabricated cleanup receipt.
use super::*;
use crate::{desktop_launch::DesktopLaunchRouter, local::PrivateRequest};
use shellfox_protocol::automation::AutomationStart;
use std::{collections::BTreeMap, time::Instant};

#[test]
#[ignore = "requires explicit owned raw helper; private fixture only, no user process"]
fn failed_source_warning_and_opaque_evidence_survive_service_to_actor_outcome() {
    let helper = std::path::PathBuf::from(
        std::env::var_os("SHELLFOX_RAW_TEST_HELPER").expect("owned raw helper"),
    );
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root).unwrap();
    let data = tempfile::Builder::new()
        .prefix("native-warning-actor-")
        .tempdir_in(root)
        .unwrap();
    let marker = data.path().join("descendant-must-not-survive");
    let mut service = Service::open_owned(data.path(), &helper).unwrap();
    let id = uuid::Uuid::new_v4();
    let input = AutomationStart {
        request_id: id,
        executable: helper.to_str().unwrap().into(),
        args: vec!["flood-branch".into(), marker.to_str().unwrap().into()],
        cwd: data.path().to_str().unwrap().into(),
        env: BTreeMap::new(),
        title: None,
        detached: false,
    };
    let (reply, mut response) = tokio::sync::oneshot::channel();
    service.dispatch_automation(
        PrivateRequest::AutomationStart(Box::new(input)),
        1,
        reply,
        &mut DesktopLaunchRouter::new(),
    );
    response.try_recv().unwrap().unwrap();
    assert_eq!(service.terminals.len(), 1);
    let tab = *service.terminals.keys().next().unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while service.terminals[&tab]
        .runtime
        .external_status()
        .unwrap()
        .source_sequence
        .0
        == 0
    {
        service.pump();
        assert!(Instant::now() < deadline, "raw fixture produced no source");
        std::thread::sleep(Duration::from_millis(1));
    }
    // Lose the ORIGINAL producer sender. The Runtime must honestly report
    // failure; ordered Close is now rightly forbidden before Finish/failed.
    service
        .disconnect_automation_source_for_lifecycle_test(id)
        .unwrap();
    let report = loop {
        service.pump();
        match service.shutdown() {
            Ok(report) => break report,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(2)),
            Err(error) => panic!("owned failed-source cleanup did not confirm: {error}"),
        }
    };
    let crate::PresentationShutdownState::FailedSources(warnings) = &report.presentation else {
        panic!("failed source warning required")
    };
    assert_eq!(warnings.failures().len(), 1);
    assert!(warnings.failures()[0].raw_cleanup_confirmed());
    assert!(warnings.failures()[0].failure().pending_bytes() > 0);
    assert!(!marker.exists());
    let finished = finish(service); // Actor path must carry the same sealed report.
    assert_eq!(finished, report);
    assert!(matches!(
        finished.activity,
        ActivityShutdownState::Committed
    ));
    let debug = format!("{finished:?}");
    assert!(debug.contains("failed_sources"));
    assert!(
        !debug.contains("flood-branch") && !debug.contains(&helper.to_string_lossy().to_string())
    );
}
