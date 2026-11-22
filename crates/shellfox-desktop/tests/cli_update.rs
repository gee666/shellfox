//! `shellfox update` reaches the desktop binary as `--cli-update`. Its output
//! must reach the CLI's stdio even from the Windows GUI-subsystem binary.
use std::process::Command;

fn run(args: &[&str]) -> (Option<i32>, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_shellfox"))
        .args(args)
        .output()
        .unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn development_builds_report_an_unsupported_installation() {
    let (code, stdout, stderr) = run(&["--cli-update", "--check"]);
    assert_eq!(code, Some(3), "{stdout}{stderr}");
    assert_eq!(
        stdout.trim(),
        concat!("Installed Shellfox version: ", env!("SHELLFOX_VERSION"))
    );
    assert!(stderr.contains("github.com/gee666/shellfox/releases"), "{stderr}");
}

#[test]
fn unknown_update_arguments_are_usage_errors() {
    let (code, _, stderr) = run(&["--cli-update", "--json"]);
    assert_eq!(code, Some(2));
    assert_eq!(stderr.trim(), "Usage: shellfox update [--check]");
}
