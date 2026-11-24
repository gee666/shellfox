#![cfg(all(
    any(target_os = "linux", target_os = "macos"),
    feature = "test-support"
))]
use shellfox_terminal::raw_process::{RawOutput, RawProcess, RawStream};
use std::time::{Duration, Instant};
#[path = "support/raw_unix.rs"]
mod support;
use support::*;
#[path = "support/raw_unix_common.rs"]
mod common;
#[path = "support/raw_wake.rs"]
mod wake;
#[path = "support/wakes.rs"]
mod wakes;

#[test]
fn actual_guest_codes_are_not_controller_cleanup_sigkill() {
    let fixture = Fixture::new();
    for code in [0, 7, 137, 255] {
        let mut process = fixture.spawn(&["exit-code", &code.to_string()]);
        assert!(process.pid() > 0);
        assert_eq!(collect(&mut process).2, code);
        assert_eq!(process.exit_code(), Some(code));
        assert!(!process.was_forcibly_stopped());
    }
}
#[test]
fn actual_guest_signal9_and_requested_stop_have_real137_status() {
    let fixture = Fixture::new();
    let mut process = fixture.spawn(&["unix-self-kill"]);
    assert_eq!(collect(&mut process).2, 137);
    assert!(!process.was_forcibly_stopped());
    let mut process = fixture.spawn(&["idle"]);
    process.request_stop().unwrap();
    assert_eq!(collect(&mut process).2, 137);
    assert!(process.was_forcibly_stopped());
}
fn tree(signal: Option<i32>, stop: bool) {
    let fixture = Fixture::new();
    let socket = fixture.socket();
    assert!(!socket.exists());
    let mut process = fixture.spawn(&[
        "unix-branch",
        socket.to_str().unwrap(),
        if signal.is_none() && !stop {
            "natural"
        } else {
            "idle"
        },
    ]);
    let leaf = pin_leaf(&socket);
    assert!(!leaf.exited());
    if let Some(signal) = signal {
        process.signal_controller_for_test(signal).unwrap();
    } else if stop {
        process.request_stop().unwrap();
    } else {
        process.write(vec![42], true).unwrap().wait().unwrap();
    }
    // No host polling or shutdown can help this observation. The persistent
    // controller must wait the real guest then kill its exact cgroup itself.
    until(|| leaf.exited());
    let code = collect(&mut process).2;
    assert_eq!(code, if signal.is_none() && !stop { 7 } else { 137 });
    assert_eq!(process.was_forcibly_stopped(), stop);
    std::fs::remove_file(socket).unwrap();
}
#[test]
fn natural_raw_exit_cleans_setsid_descendant_before_host_poll() {
    tree(None, false);
}
#[test]
fn raw_stop_cleans_setsid_descendant_before_host_poll() {
    tree(None, true);
}
#[test]
fn raw_controller_hup_retains_cleanup_and_reports_actual_guest137() {
    tree(Some(libc::SIGHUP), false);
}
#[test]
fn raw_controller_term_retains_cleanup_and_reports_actual_guest137() {
    tree(Some(libc::SIGTERM), false);
}
#[test]
fn raw_controller_int_retains_cleanup_and_reports_actual_guest137() {
    tree(Some(libc::SIGINT), false);
}
#[test]
fn lost_original_controller_never_invents_guest137_or_finish() {
    let fixture = Fixture::new();
    let socket = fixture.socket();
    // Without a cgroup nothing catches a setsid escape after a controller crash.
    #[cfg(target_os = "linux")]
    let escape = if shellfox_terminal::unix::containment()
        == shellfox_terminal::unix::Containment::Session
    {
        "setpgid"
    } else {
        "setsid"
    };
    #[cfg(not(target_os = "linux"))]
    let escape = "setpgid";
    let mut process = fixture.spawn(&["unix-branch", socket.to_str().unwrap(), "idle", escape]);
    let leaf = pin_leaf(&socket);
    process.signal_controller_for_test(libc::SIGKILL).unwrap();
    let mut unconfirmed = false;
    until(|| {
        match process.poll_exit() {
            Ok(status) => assert!(status.is_none(), "controller kill is not a guest exit"),
            Err(_) => unconfirmed = true,
        }
        unconfirmed && leaf.exited()
    });
    assert_eq!(process.exit_code(), None);
    assert!(!process.was_forcibly_stopped());
    process.close().unwrap();
    assert_eq!(process.exit_code(), None);
    std::fs::remove_file(socket).unwrap();
}
#[test]
fn raw_restart_has_fresh_pipe_state_after_actual_eof_exit() {
    let fixture = Fixture::new();
    for bytes in [vec![0, 255, 1, 0], vec![2, 0, 128, 254]] {
        let mut process = fixture.spawn(&["echo"]);
        process.write(bytes.clone(), true).unwrap().wait().unwrap();
        let (stdout, stderr, code) = collect(&mut process);
        assert_eq!(stdout, bytes);
        assert_eq!(stderr, [0, 255, 10]);
        assert_eq!(code, 7);
    }
}
#[test]
fn raw_target_closed_stdin_is_actual_epipe_not_queue_admission() {
    use shellfox_terminal::raw_process::AutomationStdinClosed;
    let fixture = Fixture::new();
    let mut process = fixture.spawn(&["unix-close-input"]);
    let mut observed = Vec::new();
    until(|| {
        while let Some(RawOutput::Data { bytes, .. }) = process.try_output() {
            observed.extend(bytes);
        }
        observed
            .windows(b"RAW_INPUT_CLOSED".len())
            .any(|b| b == b"RAW_INPUT_CLOSED")
    });
    let error = process.write(vec![42], false).unwrap().wait().unwrap_err();
    let closed = error
        .downcast_ref::<AutomationStdinClosed>()
        .expect("actual OS EPIPE");
    assert_eq!(closed.io_error().unwrap().raw_os_error(), Some(libc::EPIPE));
    process.request_stop().unwrap();
    assert_eq!(collect(&mut process).2, 137);
}
#[test]
fn canceled_partial_raw_write_never_becomes_success_or_known_closed() {
    use shellfox_terminal::raw_process::{AutomationStdinClosed, AutomationStdinIoError};
    let fixture = Fixture::new();
    let mut process = fixture.spawn(&["idle"]);
    let mut receipts = Vec::new();
    for _ in 0..32 {
        match process.write(vec![42; 49152], false) {
            Ok(receipt) => receipts.push(receipt),
            Err(_) => break,
        }
    }
    std::thread::sleep(Duration::from_millis(40));
    let pending: Vec<_> = receipts
        .into_iter()
        .filter(|receipt| receipt.try_result().unwrap().is_none())
        .collect();
    assert!(
        !pending.is_empty(),
        "fill the real pipe, not an assumed pipe size"
    );
    process.release_stdin();
    let mut actual_cancellation = false;
    for receipt in pending {
        let error = receipt.wait().unwrap_err();
        assert!(error.downcast_ref::<AutomationStdinClosed>().is_none());
        if let Some(error) = error.downcast_ref::<AutomationStdinIoError>() {
            assert_eq!(error.io_error().raw_os_error(), Some(libc::ECANCELED));
            actual_cancellation = true;
        }
    }
    assert!(
        actual_cancellation,
        "the original active OS write must be canceled"
    );
    assert!(process.poll_exit().unwrap().is_none());
    process.request_stop().unwrap();
    assert_eq!(collect(&mut process).2, 137);
}
#[test]
fn raw_sealed_facts_prove_same_account_non_elevating_launch() {
    use shellfox_terminal::launch_privilege::*;
    let fixture = Fixture::new();
    let (binding, verifier) = new_launch_binding();
    let mut process =
        RawProcess::spawn_with_launch_binding(&fixture.helper, fixture.command(&["idle"]), binding)
            .unwrap();
    let evidence = process.take_launch_evidence().unwrap();
    assert!(verifier.matches(&evidence));
    let facts = evidence.facts();
    assert_eq!(facts.purpose, LaunchFactoryPurpose::RawProgram);
    assert_eq!(facts.stage, LaunchEvidenceStage::BeforeRawGoConfirmedReady);
    assert_eq!(facts.privilege, LaunchPrivilege::Normal);
    assert!(facts.unknown.is_none());
    assert!(facts.token.is_none());
    let uid = unsafe { libc::getuid() };
    #[cfg(target_os = "linux")]
    {
        assert_eq!(
            facts.basis,
            LaunchEvidenceBasis::OriginalPinnedPosixCredentials
        );
        assert!(facts.posix.unwrap().is_non_elevating_account(uid));
    }
    #[cfg(target_os = "macos")]
    {
        assert_eq!(
            facts.basis,
            LaunchEvidenceBasis::OriginalDarwinCredentials { uid }
        );
        assert!(facts.posix.is_none());
    }
    process.request_stop().unwrap();
    assert_eq!(collect(&mut process).2, 137);
}
#[test]
fn retained_raw_failure_does_not_publish_launch_evidence_or_accept_input() {
    use shellfox_terminal::launch_privilege::*;
    let fixture = Fixture::new();
    let socket = fixture.socket();
    let (binding, _) = new_launch_binding();
    let error = RawProcess::spawn_fault_with_launch_binding(
        &fixture.helper,
        fixture.command(&["unix-branch", socket.to_str().unwrap(), "idle"]),
        binding,
        false,
        false,
        true,
    )
    .unwrap_err();
    let mut retained = error.take_process().expect("original resources retained");
    let leaf = pin_leaf(&socket);
    assert!(!leaf.exited());
    assert!(retained.take_launch_evidence().is_none());
    assert!(retained.write(vec![42], false).is_err());
    retained.close().unwrap();
    until(|| leaf.exited());
    std::fs::remove_file(socket).unwrap();
}
#[test]
fn raw_path_search_and_exec_failures_do_not_fallback_to_a_shell() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let bin = fixture.directory.join("bin space");
    std::fs::create_dir(&bin).unwrap();
    let executable = bin.join("literal $name");
    std::fs::copy(&fixture.helper, &executable).unwrap();
    let mut command = fixture.command(&["describe", "literal $HOME; & false"]);
    command.executable = "literal $name".into();
    command
        .env
        .insert("PATH".into(), bin.to_str().unwrap().into());
    let mut process = RawProcess::spawn(&fixture.helper, command).unwrap();
    let (stdout, _, code) = collect(&mut process);
    assert_eq!(code, 0);
    let value: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(value["args"], serde_json::json!(["literal $HOME; & false"]));
    let invalid = fixture.directory.join("not-an-executable-format");
    std::fs::write(&invalid, b"not an ELF and no shebang").unwrap();
    std::fs::set_permissions(&invalid, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut command = fixture.command(&[]);
    command.executable = invalid.to_str().unwrap().into();
    let error = RawProcess::spawn(&fixture.helper, command).unwrap_err();
    assert!(error.take_process().is_none(), "{error:#}");
    let script = fixture.directory.join("explicit-shebang");
    std::fs::write(&script, b"#!/bin/sh\nprintf explicit-interpreter\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut command = fixture.command(&[]);
    command.executable = script.to_str().unwrap().into();
    let mut process = RawProcess::spawn(&fixture.helper, command).unwrap();
    assert_eq!(collect(&mut process).0, b"explicit-interpreter");
}
#[test]
fn stop_of_a_finishing_flood_is_always_the_guests_sigkill() {
    stop_of_a_finishing_flood_is_the_guests_sigkill(20);
}
