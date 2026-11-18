#![cfg(all(windows, feature = "test-support"))]
#[path = "support/pty.rs"]
mod support;
use shellfox_terminal::PtySession;
use std::{
    ffi::OsString,
    thread,
    time::{Duration, Instant},
};
use support::{drain, Scratch};

#[test]
fn draining_close_preserves_queued_output_and_retains_failed_cleanup() {
    let scratch = Scratch::new();
    let mut session = PtySession::spawn(scratch.config("--short")).unwrap();
    thread::sleep(Duration::from_millis(100));
    session.begin_shutdown().unwrap();
    assert!(session.write(b"after begin close").is_err());
    assert!(session.resize(80, 24).is_err());
    let output = drain(&mut session);
    assert!(String::from_utf8_lossy(&output).contains("PTY_FINAL_MARKER"));
    session.shutdown().unwrap();
    session.begin_shutdown().unwrap();

    let mut session = PtySession::spawn(scratch.config("--idle")).unwrap();
    session.set_cleanup_failure_for_test(true);
    assert!(session.begin_shutdown().is_err());
    assert!(session.write(b"disabled after failure").is_err());
    assert!(session.resize(80, 24).is_err());
    assert!(session.try_wait().unwrap().is_none());
    session.set_cleanup_failure_for_test(false);
    session.begin_shutdown().unwrap();
    drain(&mut session);
    session.shutdown().unwrap();
}

#[test]
fn real_short_lived_pty_preserves_output_and_exit_code() {
    let scratch = Scratch::new();
    let mut session = PtySession::spawn(scratch.config("--short")).unwrap();
    let output = drain(&mut session);
    assert!(String::from_utf8_lossy(&output).contains("PTY_FINAL_MARKER"));
    assert_eq!(session.try_wait().unwrap().unwrap().exit_code(), 7);
    session.shutdown().unwrap();
    session.shutdown().unwrap();
    assert!(session.write(b"after close").is_err());
    assert!(session.resize(80, 24).is_err());
}

#[test]
fn ready_session_accepts_input_resize_and_uses_isolated_environment() {
    let scratch = Scratch::new();
    // No process-global environment mutation. The Cargo parent normally supplies
    // CARGO_MANIFEST_DIR, which must not reach a cleared child environment.
    let mut config = scratch.config("--echo");
    config.env.insert("CUSTOM_EXPERIMENT".into(), "yes".into());
    let mut session = PtySession::spawn(config).unwrap();
    session.resize(97, 31).unwrap();
    assert!(session.resize(0, 31).is_err());
    session.write(b"hello-owned-pty\r").unwrap();
    let output = String::from_utf8_lossy(&drain(&mut session)).into_owned();
    assert!(output.contains("REPLY=hello-owned-pty"), "{output}");
    assert!(output.contains("SECRET=absent"), "{output}");
    assert!(
        output.contains(&format!("HOME={}", scratch.0.join("home").display())),
        "{output}"
    );
    session.shutdown().unwrap();
}

#[test]
fn bounded_reader_preserves_large_final_output_and_cancels_full_queue() {
    let scratch = Scratch::new();
    let mut session = PtySession::spawn(scratch.config("--flood")).unwrap();
    thread::sleep(Duration::from_millis(100));
    let bytes = drain(&mut session);
    assert!(bytes.len() >= 600 * 4096);
    assert!(String::from_utf8_lossy(&bytes).contains("FLOOD_FINAL"));
    session.shutdown().unwrap();
    let mut session = PtySession::spawn(scratch.config("--flood")).unwrap();
    thread::sleep(Duration::from_millis(100));
    let start = Instant::now();
    session.shutdown().unwrap();
    assert!(start.elapsed() < Duration::from_secs(6));
}

#[test]
fn natural_root_exit_terminates_remaining_job_descendants_only() {
    let scratch = Scratch::new();
    let unrelated = Scratch::new();
    let mut independent =
        std::process::Command::new(env!("CARGO_BIN_EXE_shellfox-pty-test-helper"))
            .arg("--delayed-child")
            .current_dir(&unrelated.0)
            .spawn()
            .unwrap();
    let mut session = PtySession::spawn(scratch.config("--leave-child")).unwrap();
    drain(&mut session);
    session.shutdown().unwrap();
    assert!(independent.wait().unwrap().success());
    assert!(unrelated.0.join("descendant-survived").exists());
    assert!(!scratch.0.join("descendant-survived").exists());
}

#[test]
fn preflight_and_partial_startup_fail_closed() {
    let scratch = Scratch::new();
    let mut config = scratch.config("--idle");
    config.supervisor = None;
    assert!(PtySession::spawn(config)
        .unwrap_err()
        .take_session()
        .is_none());
    let mut config = scratch.config("--idle");
    config.command.as_mut().unwrap().program =
        scratch.0.join("missing-command.exe").into_os_string();
    assert!(PtySession::spawn(config)
        .unwrap_err()
        .take_session()
        .is_none());
    let invalid = scratch.0.join("not-executable.txt");
    std::fs::write(&invalid, "not an executable").unwrap();
    let mut config = scratch.config("--idle");
    config.supervisor = Some(invalid);
    assert!(PtySession::spawn(config)
        .unwrap_err()
        .take_session()
        .is_none());
}

#[test]
fn close_during_active_output_and_repeated_close_are_safe() {
    let scratch = Scratch::new();
    for _ in 0..3 {
        let mut session = PtySession::spawn(scratch.config("--flood")).unwrap();
        session.shutdown().unwrap();
        session.shutdown().unwrap();
    }
}

#[test]
fn injected_assignment_denial_never_releases_the_supervisor() {
    let scratch = Scratch::new();
    let error = PtySession::spawn_with_test_fault(
        scratch.config("--delayed-child"),
        shellfox_terminal::StartupFault::AssignmentDenied,
    )
    .unwrap_err();
    assert!(
        error.take_session().is_none(),
        "trusted root cleanup must be confirmed"
    );
    assert!(!scratch.0.join("descendant-survived").exists());
}

#[test]
fn missing_go_times_out_without_starting_the_command() {
    let scratch = Scratch::new();
    let start = Instant::now();
    let error = PtySession::spawn_with_test_fault(
        scratch.config("--delayed-child"),
        shellfox_terminal::StartupFault::NoRelease,
    )
    .unwrap_err();
    assert!(start.elapsed() < Duration::from_secs(11));
    assert!(error.take_session().is_none());
    assert!(!scratch.0.join("descendant-survived").exists());
}

#[test]
fn failed_startup_and_shutdown_retain_owners_and_disable_input_until_retry() {
    let scratch = Scratch::new();
    let error = PtySession::spawn_with_test_fault(
        scratch.config("--idle"),
        shellfox_terminal::StartupFault::ReadyCleanupFailure,
    )
    .unwrap_err();
    let mut session = error
        .take_session()
        .expect("failed cleanup must retain the controller");
    assert!(error.take_session().is_none());
    assert!(session.write(b"must not reach shell\r").is_err());
    assert!(session.resize(80, 24).is_err());
    assert!(session.shutdown().is_err());
    session.set_cleanup_failure_for_test(false);
    session.shutdown().unwrap();
    session.shutdown().unwrap();

    let mut session = PtySession::spawn(scratch.config("--idle")).unwrap();
    session.set_cleanup_failure_for_test(true);
    assert!(session.shutdown().is_err());
    assert!(session.write(b"blocked after failed close\r").is_err());
    session.set_cleanup_failure_for_test(false);
    session.shutdown().unwrap();
}

#[test]
fn direct_supervisor_invocation_without_valid_parent_gate_cannot_spawn() {
    let scratch = Scratch::new();
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_shellfox-pty-test-helper"))
        .args([
            OsString::from("--shellfox-pty-supervisor"),
            "malformed".into(),
            "malformed".into(),
            env!("CARGO_BIN_EXE_shellfox-pty-test-helper").into(),
            "--delayed-child".into(),
        ])
        .current_dir(&scratch.0)
        .status()
        .unwrap();
    assert!(!status.success());
    assert!(!scratch.0.join("descendant-survived").exists());
}
