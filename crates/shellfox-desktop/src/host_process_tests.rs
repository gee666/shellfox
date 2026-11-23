use super::*;

#[test]
fn digest_is_lowercase_hex() {
    assert_eq!(hex(&[0x00, 0xab, 0x7f]), "00ab7f");
}

#[test]
fn owner_marker_round_trips_and_rejects_garbage() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    let directory = tempfile::tempdir_in(root).unwrap();
    assert_eq!(recorded_owner(directory.path()), None);
    let owner: LaunchOwnerFence = "12345678-1234-4234-8234-123456789abc".parse().unwrap();
    record_owner(directory.path(), owner).unwrap();
    assert_eq!(recorded_owner(directory.path()), Some(owner));
    std::fs::write(directory.path().join(OWNER_MARKER), "not a fence").unwrap();
    assert_eq!(recorded_owner(directory.path()), None);
}

#[test]
fn missing_cli_fails_at_once_and_names_the_cli() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().join("tmp");
    std::fs::create_dir_all(&root).unwrap();
    let directory = tempfile::tempdir_in(root).unwrap();
    let executable = directory.path().join(if cfg!(windows) { "shellfox.exe" } else { "shellfox" });
    let config = HostEndpointConfig {
        data_dir: directory.path().join("data"),
        helper: directory.path().join("helper"),
        helper_sha256: [0; 32],
    };
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let began = Instant::now();
    let Err(error) = start_and_discover(&runtime, &executable, &[], &[], &config) else {
        panic!("a host started without its CLI");
    };
    assert!(began.elapsed() < Duration::from_secs(5), "retried a missing CLI for {:?}", began.elapsed());
    assert!(format!("{error:#}").contains("shellfox-cli"), "unclear error: {error:#}");
    assert!(!config.data_dir.exists());
}

#[cfg(target_os = "linux")]
#[test]
fn linux_host_scope_survives_oom_kills_of_its_members() {
    let arguments = platform::scope_arguments(&uuid::Uuid::nil());
    assert!(arguments.contains(&"--property=OOMPolicy=continue".to_owned()));
    assert!(arguments.contains(&"--scope".to_owned()) && arguments.contains(&"--user".to_owned()));
    assert_eq!(arguments.last().map(String::as_str), Some("--"));
}
