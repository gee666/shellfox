use super::*;
fn program(name: &str) -> PathBuf {
    if cfg!(windows) { PathBuf::from(r"D:\producer-programs").join(name) }
    else { PathBuf::from("/producer-programs").join(name) }
}
#[test]
fn no_inputs_do_not_discover_programs_or_touch_the_helper() {
    assert!(SshArguments::default().finish(Path::new("missing-relative-helper"), [0;32]).unwrap().is_none());
}
#[test]
fn all_roles_require_explicit_complete_pairs_without_file_io() {
    let mut args = SshArguments::default();
    for flag in ["--ssh-program", "--ssh-keyscan", "--ssh-askpass-cli"] {
        assert!(super::super::ApplicationArguments::is_flag(std::ffi::OsStr::new(flag)));
        args.push(flag, program(flag).into_os_string()).unwrap();
        args.push(&format!("{flag}-sha256"), "ab".repeat(32).into()).unwrap();
    }
    assert!(!args.is_empty());
    for pair in [args.ssh, args.keyscan, args.askpass] {
        let (path, digest) = pair.complete().unwrap();
        assert!(path.is_absolute());
        assert_eq!(digest, [0xab;32]);
    }
    // These parser-only paths/hashes were not supplied to a resource constructor.
}
#[test]
fn incomplete_duplicate_traversing_or_unknown_inputs_fail_before_acquisition() {
    let mut args = SshArguments::default();
    args.push("--ssh-program", program("ssh").into_os_string()).unwrap();
    assert!(args.push("--ssh-program", program("ssh").into_os_string()).is_err());
    assert!(args.finish(Path::new("missing-helper"), [0;32]).is_err());
    for value in ["bad".into(), "g".repeat(64)] {
        assert!(SshArguments::default().push("--ssh-program-sha256", value.into()).is_err());
    }
    assert!(SshArguments::default().push("--ssh-program", "relative-ssh".into()).is_err());
    assert!(SshArguments::default().push("--ssh-program", program("../escape").into_os_string()).is_err());
    assert!(SshArguments::default().push("--ssh-program-sha256-sha256", "a".repeat(64).into()).is_err());
}
