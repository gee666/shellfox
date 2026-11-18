#![cfg(windows)]
mod support;
use shellfox_runtime::*;
use shellfox_terminal::inspection::InspectionPlatform;
use std::time::{Duration, Instant};
use support::Harness;

#[test]
fn launched_command_and_owned_descendant_are_inspected_without_blocking_typing() {
    let mut host = Harness::start("--child-inspection");
    host.until(|h| {
        let output = String::from_utf8_lossy(&h.output);
        output.contains("INSPECTION_ROOT_READY") && output.contains("OWNED_AGENT_READY")
    });
    let ticket = host.runtime.request_inspection().unwrap();
    assert_eq!(
        host.runtime.request_inspection(),
        Err(InspectionFailure::Pending)
    );
    // This is the ordinary command/event path while inspection runs elsewhere.
    host.request(Command::RawInput {
        lease: host.lease,
        bytes: b"typing-still-responsive\r".to_vec(),
    })
    .unwrap();
    host.until(|h| {
        String::from_utf8_lossy(&h.output).contains("INSPECTION_TYPING=typing-still-responsive")
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let snapshot = loop {
        assert!(
            Instant::now() < deadline,
            "bounded inspection did not finish"
        );
        match host.runtime.poll_inspection(ticket) {
            InspectionPoll::Ready(Ok(snapshot)) => break snapshot,
            InspectionPoll::Ready(Err(error)) => panic!("owned inspection failed: {error}"),
            InspectionPoll::Pending => std::thread::sleep(Duration::from_millis(2)),
        }
    };
    assert_eq!(snapshot.identity, host.runtime.identity());
    let evidence = snapshot.evidence;
    assert!(evidence.platform == InspectionPlatform::Windows);
    assert!(evidence.root_alive && evidence.complete);
    assert!(
        evidence.processes.len() >= 2
            && evidence.processes.len() <= shellfox_terminal::inspection::MAX_INSPECTION_PROCESSES
    );
    let roots: Vec<_> = evidence
        .processes
        .iter()
        .filter(|row| row.is_root)
        .collect();
    assert_eq!(roots.len(), 1);
    let root = roots[0];
    assert!(root.identity == evidence.root);
    assert!(root.identity.pid != std::process::id() && root.identity.birth != 0);
    assert!(root.accessible);
    let root_args = root.argv.as_ref().expect("exact original command argv");
    assert!(root_args.iter().any(|arg| arg == "--child-inspection"));
    assert!(!root_args
        .iter()
        .any(|arg| arg == "--shellfox-pty-supervisor"));
    assert!(evidence
        .processes
        .iter()
        .filter(|row| !row.is_root)
        .any(|row| row.accessible
            && row
                .argv
                .as_ref()
                .is_some_and(|argv| argv.iter().any(|arg| arg == "--owned-agent"))));
    for row in &evidence.processes {
        assert!(row.identity.pid != std::process::id());
        assert!(!row
            .argv
            .as_ref()
            .is_some_and(|argv| argv.iter().any(|arg| arg == "--shellfox-pty-supervisor")));
    }
    host.request(Command::Close { lease: host.lease }).unwrap();
    host.until(|h| h.exited);
    assert_eq!(
        host.runtime.request_inspection(),
        Err(InspectionFailure::Unavailable)
    );
    assert!(matches!(
        host.runtime.poll_inspection(ticket),
        InspectionPoll::Ready(Err(InspectionFailure::Unavailable))
    ));
}
