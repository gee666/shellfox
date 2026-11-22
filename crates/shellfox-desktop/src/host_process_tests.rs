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

#[cfg(target_os = "linux")]
#[test]
fn linux_host_scope_survives_oom_kills_of_its_members() {
    let arguments = platform::scope_arguments(&uuid::Uuid::nil());
    assert!(arguments.contains(&"--property=OOMPolicy=continue".to_owned()));
    assert!(arguments.contains(&"--scope".to_owned()) && arguments.contains(&"--user".to_owned()));
    assert_eq!(arguments.last().map(String::as_str), Some("--"));
}
