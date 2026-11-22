#![cfg(all(windows, feature = "test-support"))]
#[path = "support/elevated_hold.rs"]
mod hold;
#[path = "support/elevated_fixture.rs"]
mod support;
use sha2::{Digest, Sha256};
use shellfox_terminal::elevated::{
    fixture_session, AdapterError, Event, FixtureScenario, Generation, TrustedHash,
};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use support::Fixture;
const CHILD_ROOT: &str = "SHELLFOX_OWNED_ELEVATED_RETENTION_TEST";

#[test]
fn unconfirmed_peer_keeps_resource_hold_until_test_host_process_death() {
    let fixture = Fixture::new();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "unconfirmed_retention_child",
            "--nocapture",
        ])
        .env_clear()
        .env(CHILD_ROOT, &fixture.root)
        .env("TMP", &fixture.root)
        .env("TEMP", &fixture.root)
        .stdin(Stdio::null());
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() >= deadline {
            // Only this exact owned TEST HOST. Its fake scenario creates no
            // peer/shell. This is not a production cleanup path or PID fallback.
            let _ = child.kill();
            let _ = child.wait();
            panic!("owned retention test host timed out");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    // The child deliberately used process death, not a fabricated close ACK, to
    // end this test. Only now can the outer fixture remove its private files.
}

#[test]
#[ignore = "owned subprocess entrypoint, invoked by the parent retention test"]
fn unconfirmed_retention_child() {
    let Some(root) = std::env::var_os(CHILD_ROOT).map(PathBuf::from) else {
        return;
    };
    let temporary =
        std::fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp")).unwrap();
    let canonical = std::fs::canonicalize(&root).unwrap();
    assert!(canonical.starts_with(temporary));
    assert!(root
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("elevated-stdio-"));
    let hash = TrustedHash::parse(&format!(
        "{:x}",
        Sha256::digest(std::fs::read(env!("CARGO_BIN_EXE_shellfox-elevated-test-helper")).unwrap())
    ))
    .unwrap();
    let fixture = Fixture {
        image: root.join("Shellfox.ElevatedTerminal.exe"),
        root,
        hash,
    };
    let lock = hold::lock(&fixture);
    let weak = Arc::downgrade(&lock);
    let generation = Generation::new(19).unwrap();
    let mut session = fixture_session(
        fixture.verified(),
        Arc::clone(&lock),
        FixtureScenario::Unconfirmed,
        generation,
        fixture.launch(),
    )
    .unwrap();
    drop(lock);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if matches!(session.poll(generation).unwrap(), Some(Event::Ready(_))) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    while !session.observation().launcher_exited() {
        session.service(generation).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    loop {
        match session.poll(generation) {
            Err(AdapterError::CleanupUnconfirmed) => break,
            Ok(None) => {}
            other => panic!("unexpected unconfirmed fixture event {other:?}"),
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!session.cleanup_confirmed());
    assert_eq!(
        session.finish(generation, Duration::ZERO),
        Err(AdapterError::CleanupUnconfirmed)
    );
    let observation = session.observation();
    drop(session);
    assert!(observation.resources_held() && !observation.peer_exit_confirmed());
    assert!(weak.upgrade().is_some());
    assert!(std::fs::write(fixture.root.join("lazy.dll"), b"must remain locked").is_err());
    // The unconfirmed cleanup owner remains alive and keeps the hold.
    // Explicit test-host death is not an application successful-close assertion.
    std::process::exit(0);
}
