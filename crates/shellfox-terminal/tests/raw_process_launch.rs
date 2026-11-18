#![cfg(all(windows, feature = "test-support"))]
#[path = "support/raw_process.rs"]
mod support;
use shellfox_terminal::{
    launch_privilege::{
        fixture_current_token, new_launch_binding, LaunchEvidenceBasis, LaunchEvidenceStage,
        LaunchFactoryPurpose,
    },
    raw_process::{RawOutput, RawProcess},
};
use std::time::{Duration, Instant};
use support::*;
#[test]
fn standard_raw_facts_compare_actual_current_token_and_survive_confirmed_cleanup() {
    let f = Fixture::new();
    let expected = fixture_current_token().unwrap();
    let (binding, verifier) = new_launch_binding();
    let (_, wrong) = new_launch_binding();
    let mut process =
        RawProcess::spawn_with_launch_binding(&f.helper, f.command(&["describe"]), binding)
            .unwrap();
    let evidence = process
        .take_launch_evidence()
        .expect("Only successful READY returns the sealed snapshot");
    assert!(verifier.matches(&evidence));
    assert!(!wrong.matches(&evidence));
    assert!(process.take_launch_evidence().is_none());
    let facts = evidence.facts();
    assert_eq!(facts.token, Some(expected));
    assert_eq!(
        facts.purpose,
        LaunchFactoryPurpose::StandardRawCreateProcess
    );
    assert_eq!(
        facts.basis,
        LaunchEvidenceBasis::ReviewedSupervisorPrimaryTokenInheritance
    );
    assert_eq!(facts.stage, LaunchEvidenceStage::BeforeRawGoConfirmedReady);
    assert_eq!(collect(&mut process).2, 0);
    drop(process);
    // This same opaque receipt can be retained by a published final parser. It
    // is descriptive initial-launch data, not a live Job/token/share capability.
    assert_eq!(evidence.facts(), facts);
    assert!(verifier.matches(&evidence));
}
#[test]
fn old_unbound_factory_does_not_infer_privilege_from_process_or_path() {
    let f = Fixture::new();
    let mut process = f.spawn(&["describe"]);
    assert!(process.take_launch_evidence().is_none());
    assert_eq!(collect(&mut process).2, 0);
}
#[test]
fn post_ready_failed_start_keeps_raw_cleanup_owner_but_never_publishes_facts() {
    let f = Fixture::new();
    let marker = f.directory.join("facts-failed-descendant");
    let (binding, _) = new_launch_binding();
    let error = RawProcess::spawn_fault_with_launch_binding(
        &f.helper,
        f.command(&["idle-branch", marker.to_str().unwrap(), "null"]),
        binding,
        false,
        false,
        true,
    )
    .unwrap_err();
    let mut retained = error
        .take_process()
        .expect("Failed cleanup must retain original owner");
    assert!(retained.take_launch_evidence().is_none());
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if matches!(retained.try_output(), Some(RawOutput::Data { .. })) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    retained.close().unwrap();
    std::thread::sleep(Duration::from_millis(1650));
    assert!(!marker.exists());
    assert!(retained.take_launch_evidence().is_none());
}
