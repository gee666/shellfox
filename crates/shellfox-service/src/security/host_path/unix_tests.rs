use super::*;
use cap_std::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;

#[test]
fn held_directory_chmod_cannot_change_a_substituted_path() {
    let data = super::tests::scratch();
    let path = data.path().join("original");
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let plan = HostRoot::inspect(&path).unwrap();
    let held = plan.anchors.last().unwrap().handle.try_clone().unwrap();
    let user = held.dir_metadata().unwrap().uid();
    let renamed = data.path().join("retained-original");
    std::fs::rename(&path, &renamed).unwrap();
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    super::permissions::protect_directory(&held, user).unwrap();
    assert_eq!(
        std::fs::metadata(&renamed).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert!(plan.revalidate().is_err());
}
#[test]
fn readable_owned_anchor_is_allowed_but_foreign_writable_ancestor_is_rejected_before_creation() {
    let data = super::tests::scratch();
    let parent = data.path().join("readable");
    std::fs::create_dir(&parent).unwrap();
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
    let plan = HostRoot::inspect(&parent.join("new-host")).unwrap();
    plan.validate_creation_ancestors().unwrap();
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(plan.validate_creation_ancestors().is_err());
    assert!(!parent.join("new-host").exists());
}
#[cfg(target_os = "linux")]
#[test]
fn an_ancestor_acl_is_refused_only_when_it_lets_another_user_write() {
    let data = super::tests::scratch();
    let parent = data.path().join("shared");
    std::fs::create_dir(&parent).unwrap();
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
    // Linux CI installs the acl package. Any account other than the test's.
    let other = if rustix::process::getuid().is_root() {
        65534
    } else {
        0
    };
    let setfacl = |spec: &str| {
        let status = std::process::Command::new("setfacl")
            .args(["-m", spec])
            .arg(&parent)
            .status()
            .expect("setfacl from the acl package");
        assert!(status.success());
    };
    setfacl(&format!("u:{other}:rx"));
    let plan = HostRoot::inspect(&parent.join("new-host")).unwrap();
    plan.validate_creation_ancestors().unwrap();
    setfacl(&format!("u:{other}:rwx"));
    assert!(plan.validate_creation_ancestors().is_err());
    assert!(!parent.join("new-host").exists());
}
