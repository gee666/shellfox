#![cfg(all(target_os = "linux", feature = "test-support"))]
#[path = "support/unix_production.rs"]
mod support;
use shellfox_terminal::{
    launch_privilege::{self, LaunchEvidenceBasis, LaunchPrivilege},
    PtyConfig, PtyOutput, PtySession, UnixPrivilegeMode,
};
use support::*;
#[test]
fn production_input_output_resize_and_original_exit() {
    let fixture = Fixture::new();
    let config = fixture.shell("printf 'READY-INPUT\\n'; read value; stty size; printf 'accepted:%s\\n' \"$value\"; exit 7");
    let mut session = PtySession::spawn(config).unwrap();
    session.resize(103, 37).unwrap();
    session.write(b"hello-owned\n").unwrap();
    let (output, code) = collect(&mut session);
    let text = String::from_utf8_lossy(&output);
    assert!(text.contains("37 103"), "{text}");
    assert!(text.contains("accepted:hello-owned"), "{text}");
    assert_eq!(code, 7);
}
#[test]
fn production_default_shell_is_interactive() {
    let fixture = Fixture::new();
    let mut config = PtyConfig::isolated(&fixture.root);
    config.supervisor = Some(helper());
    let mut session = PtySession::spawn(config).unwrap();
    session
        .write(b"printf 'default-shell-ok\\n'; exit 0\n")
        .unwrap();
    let (output, code) = collect(&mut session);
    assert!(String::from_utf8_lossy(&output).contains("default-shell-ok"));
    assert_eq!(code, 0);
}
#[test]
fn production_failed_exec_never_reports_ready_and_confirms_cleanup() {
    let fixture = Fixture::new();
    let config = fixture.config(&fixture.root.join("missing-command"), &[]);
    let error = PtySession::spawn(config).unwrap_err();
    assert!(error.take_session().is_none(), "{error:#}");
    assert!(error.to_string().contains("START"), "{error:#}");
}
#[test]
fn production_non_elevating_facts_are_sealed_and_inherited() {
    let fixture = Fixture::new();
    let mut config = fixture.shell("grep '^NoNewPrivs:' /proc/self/status; exit 0");
    config.unix_privilege = UnixPrivilegeMode::NonElevating;
    let (binding, verifier) = launch_privilege::new_launch_binding();
    let mut session = PtySession::spawn_bound(config, binding).unwrap();
    let evidence = session.take_launch_evidence().unwrap();
    assert!(verifier.matches(&evidence));
    let facts = evidence.facts();
    assert_eq!(facts.privilege, LaunchPrivilege::Normal);
    assert_eq!(
        facts.basis,
        LaunchEvidenceBasis::OriginalPinnedPosixCredentials
    );
    assert!(facts.token.is_none());
    let posix = facts.posix.unwrap();
    assert_eq!(posix.uid, unsafe { libc::getuid() });
    assert_eq!(posix.uid, posix.euid);
    assert!(posix.no_new_privs);
    assert_eq!(
        posix.effective_caps | posix.permitted_caps | posix.inheritable_caps | posix.ambient_caps,
        0
    );
    let (output, code) = collect(&mut session);
    assert!(String::from_utf8_lossy(&output).contains("NoNewPrivs:\t1"));
    assert_eq!(code, 0);
}
#[test]
fn production_local_policy_does_not_silently_disable_sudo() {
    let fixture = Fixture::new();
    let (binding, verifier) = launch_privilege::new_launch_binding();
    let mut session = PtySession::spawn_bound(
        fixture.shell("grep '^NoNewPrivs:' /proc/self/status"),
        binding,
    )
    .unwrap();
    let evidence = session.take_launch_evidence().unwrap();
    assert!(verifier.matches(&evidence));
    assert_eq!(evidence.facts().privilege, LaunchPrivilege::Unknown);
    assert!(!evidence.facts().posix.unwrap().no_new_privs);
    let (output, _) = collect(&mut session);
    assert!(String::from_utf8_lossy(&output).contains("NoNewPrivs:\t0"));
}
#[test]
fn production_root_exit_kills_detached_descendant_with_pidfd_exit_proof() {
    descendant(false, false, false, false);
}
#[test]
fn production_shutdown_kills_detached_descendant_with_pidfd_exit_proof() {
    descendant(true, false, false, false);
}
#[test]
fn production_root_exit_kills_double_fork_setsid_orphan() {
    descendant(false, true, false, false);
}
#[test]
fn production_shutdown_kills_double_fork_setsid_orphan() {
    descendant(true, true, false, false);
}
#[test]
fn production_controller_crash_still_kills_detached_descendant() {
    descendant(false, false, true, false);
}
#[test]
fn production_controller_channel_loss_kills_cgroup_without_host_polling() {
    descendant(false, false, false, true);
}
#[test]
fn production_controller_sighup_kills_detached_leaf_before_host_poll() {
    controller_signal(libc::SIGHUP);
}
#[test]
fn production_controller_sigterm_kills_detached_leaf_before_host_poll() {
    controller_signal(libc::SIGTERM);
}
#[test]
fn production_controller_sigint_kills_detached_leaf_before_host_poll() {
    controller_signal(libc::SIGINT);
}
#[test]
fn production_guest_ctrl_c_keeps_normal_signal_behavior() {
    let fixture = Fixture::new();
    let mut session = PtySession::spawn(fixture.shell(
        "trap 'printf \"guest-interrupt\\n\"; exit 23' INT; printf 'signal-ready\\n'; while :; do read value; done"
    )).unwrap();
    let mut output = Vec::new();
    until(|| {
        while let Ok(Some(PtyOutput::Data(bytes))) = session.try_read() {
            output.extend(bytes);
        }
        String::from_utf8_lossy(&output).contains("signal-ready")
    });
    session.write(b"\x03").unwrap();
    let (output, code) = collect(&mut session);
    assert_eq!(code, 23);
    assert!(String::from_utf8_lossy(&output).contains("guest-interrupt"));
}
#[test]
fn production_startup_cancel_denial_and_uncertain_cleanup_retain_real_owners() {
    use shellfox_terminal::StartupFault;
    let fixture = Fixture::new();
    let marker = fixture.root.join("must-not-start");
    for fault in [StartupFault::AssignmentDenied, StartupFault::NoRelease] {
        let config = fixture.shell(&format!("touch '{}'", marker.display()));
        let error = PtySession::spawn_with_test_fault(config, fault).unwrap_err();
        assert!(error.take_session().is_none(), "{error:#}");
        assert!(!marker.exists());
    }
    let config = fixture.shell("while :; do sleep 1; done");
    let error =
        PtySession::spawn_with_test_fault(config, StartupFault::ReadyCleanupFailure).unwrap_err();
    let mut retained = error
        .take_session()
        .expect("uncertain cleanup must return actual owner");
    assert!(retained.write(b"not-ready").is_err());
    assert!(retained.shutdown().is_err());
    retained.set_cleanup_failure_for_test(false);
    retained.shutdown().unwrap();
    assert!(error.take_session().is_none());
}
#[test]
fn production_drop_retains_owner_until_real_cleanup() {
    let fixture = Fixture::new();
    let mut session = PtySession::spawn(fixture.shell("while :; do printf x; done")).unwrap();
    session.write(b"ignored\n").unwrap();
    drop(session);
}
