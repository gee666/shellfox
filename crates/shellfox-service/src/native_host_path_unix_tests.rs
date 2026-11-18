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
