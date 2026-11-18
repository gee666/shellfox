#![cfg(all(target_os = "linux", feature = "test-support"))]
#[path = "support/unix_production.rs"]
mod support;
use shellfox_terminal::{
    launch_privilege::{self, LaunchEvidenceBasis, LaunchPrivilege},
    AskpassBroker, PtyConfig, PtyOutput, PtySession, TrustedProgramEnv, TrustedProgramLease,
    UnixPrivilegeMode,
};
use std::{fs, path::Path, sync::Arc};
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
    let mut config = PtyConfig::new(&fixture.root);
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
fn production_trusted_ssh_carrier_is_target_only_and_revalidated() {
    let fixture = Fixture::new();
    let program: Arc<dyn TrustedProgramLease> = Arc::new(Lease::new("/bin/sh"));
    let askpass_path = fixture.root.join("leased-askpass");
    fs::write(&askpass_path, "#!/bin/sh\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&askpass_path, fs::Permissions::from_mode(0o700)).unwrap();
    let askpass: Arc<dyn TrustedProgramLease> = Arc::new(Lease::new(&askpass_path));
    let _broker_socket =
        std::os::unix::net::UnixListener::bind(fixture.root.join("askpass.sock")).unwrap();
    let broker = || {
        AskpassBroker::new(
            fixture.root.clone(),
            fixture.root.join("askpass.sock").to_str().unwrap().into(),
            "ab".repeat(32),
        )
        .unwrap()
    };
    let environment =
        TrustedProgramEnv::ssh_askpass(program.clone(), askpass.clone(), broker()).unwrap();
    let config = fixture.shell("test \"$SSH_ASKPASS_REQUIRE\" = force && test \"$SHELLFOX_SSH_ASKPASS_TOKEN\" = \"$EXPECTED_TOKEN\" && printf carrier-ok");
    let mut config = config;
    config
        .env
        .insert("EXPECTED_TOKEN".into(), "ab".repeat(32).into());
    let mut session = PtySession::spawn_program(config.clone(), environment).unwrap();
    let (output, code) = collect(&mut session);
    assert_eq!(code, 0);
    assert!(String::from_utf8_lossy(&output).contains("carrier-ok"));
    let environment = TrustedProgramEnv::ssh_askpass(program, askpass, broker()).unwrap();
    fs::rename(&askpass_path, fixture.root.join("retired-askpass")).unwrap();
    assert!(PtySession::spawn_program(config, environment)
        .unwrap_err()
        .take_session()
        .is_none());
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
fn production_actual_ssh_proxy_receives_only_trusted_carrier() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let marker = fixture.root.join("ssh-proxy-measured");
    let proxy = fixture.root.join("askpass");
    fs::write(&proxy, format!("#!/bin/sh\n[ \"$SSH_ASKPASS_REQUIRE\" = force ] && [ \"$SHELLFOX_SSH_ASKPASS_TOKEN\" = \"{}\" ] && printf measured > '{}'\n",
        "ab".repeat(32), marker.display())).unwrap();
    fs::set_permissions(&proxy, fs::Permissions::from_mode(0o700)).unwrap();
    let program: Arc<dyn TrustedProgramLease> = Arc::new(Lease::new("/usr/bin/ssh"));
    let askpass: Arc<dyn TrustedProgramLease> = Arc::new(Lease::new(&proxy));
    let _broker_socket =
        std::os::unix::net::UnixListener::bind(fixture.root.join("broker")).unwrap();
    let broker = AskpassBroker::new(
        fixture.root.clone(),
        fixture.root.join("broker").to_str().unwrap().into(),
        "ab".repeat(32),
    )
    .unwrap();
    let environment = TrustedProgramEnv::ssh_askpass(program, askpass, broker).unwrap();
    let proxy_arg = format!("ProxyCommand={}", proxy.display());
    let mut config = fixture.config(
        Path::new("/usr/bin/ssh"),
        &[
            "-F",
            "/dev/null",
            "-o",
            "BatchMode=yes",
            "-o",
            &proxy_arg,
            "owned-fixture",
        ],
    );
    config.unix_privilege = UnixPrivilegeMode::NonElevating;
    let mut session = PtySession::spawn_program(config, environment).unwrap();
    // The owned proxy deliberately supplies no SSH server. This tests the real
    // SSH executable's carrier inheritance, not network authentication.
    let (_, code) = collect(&mut session);
    assert_eq!(code, 255);
    assert_eq!(fs::read_to_string(marker).unwrap(), "measured");
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
