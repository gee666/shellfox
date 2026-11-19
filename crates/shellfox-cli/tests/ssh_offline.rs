use std::{fs, path::Path, process::Command};

#[test]
fn ssh_offline_empty_and_help_do_not_start_or_create_a_host() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join("tmp");
    let scratch = tempfile::Builder::new()
        .prefix("ssh-cli-offline-")
        .tempdir_in(root)
        .unwrap();
    let data = scratch.path().join("absent");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_shellfox-cli"))
            .arg("--data-dir")
            .arg(&data)
            .arg("ssh")
            .args(args)
            .output()
            .unwrap()
    };
    let help = run(&["--help"]);
    assert_eq!(help.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help.stdout).contains("shellfox ssh [--profile name]"));
    let missing = run(&[]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stdout).contains("  no ssh connections yet"));
    let named = run(&["--profile", "fixture"]);
    assert_eq!(named.status.code(), Some(1));
    assert_eq!(named.stdout, missing.stdout);
    let invalid = run(&["-p", "--help"]);
    assert_eq!(invalid.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&invalid.stdout).contains("invalid ssh arguments"));
    assert!(!data.exists());

    // An endpoint that cannot be authenticated must not be treated as offline.
    let endpoint = data.join("rust-service-v1/endpoint.json");
    fs::create_dir_all(endpoint.parent().unwrap()).unwrap();
    fs::write(&endpoint, b"invalid published endpoint").unwrap();
    let refused = run(&[]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&refused.stdout).contains("no ssh connections yet"));
    assert_eq!(fs::read(&endpoint).unwrap(), b"invalid published endpoint");
}
