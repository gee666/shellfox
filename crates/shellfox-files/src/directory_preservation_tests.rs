//! Retained-ledger disposition mechanics, not private birth/kernel acceptance.
use super::*;
use cap_fs_ext::DirExt;
use cap_std::ambient_authority;
use std::{
    fs,
    path::{Path, PathBuf},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .canonicalize()
            .unwrap();
        let base = PathBuf::from(base.to_str().unwrap().trim_start_matches("\\\\?\\"));
        let path = base.join(format!("directory-preservation-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn stale_preservation_ends_owned_child_pin_but_keeps_parent_and_unproven_evidence() {
    let fixture = Fixture::new();
    let issuer =
        Arc::new(FileService::approved_root(&fixture.0, crate::Access::ReadWrite).unwrap());
    let mut keeper = issuer.directory_preparation_owner();
    let parent = Dir::open_ambient_dir(&fixture.0, ambient_authority()).unwrap();
    let parent_id = HandleIdentity::from_directory(&parent).unwrap();
    fs::create_dir(fixture.0.join("child")).unwrap();
    let child = parent.open_dir_nofollow("child").unwrap();
    let child_id = HandleIdentity::from_directory(&child).unwrap();
    {
        let mut ledger = issuer.plan_creations.lock().unwrap();
        ledger.reserve(1).unwrap();
        let slot = ledger.begin(
            parent.try_clone().unwrap(),
            parent_id.clone(),
            OsString::from("child"),
        );
        ledger.commit_created(slot, child_id.clone(), child);
        assert!(ledger.proves(&parent_id, &OsString::from("child"), &child_id));
        assert_eq!(
            ledger.created_origin(&parent_id, &OsString::from("child"), &child_id),
            Some(DirectoryCreatorOrigin::Ordinary)
        );
    }
    let original_clone = {
        let ledger = issuer.plan_creations.lock().unwrap();
        assert!(ledger
            .clone_created_matching(&parent_id, std::ffi::OsStr::new("different"), &child_id)
            .unwrap()
            .is_none());
        assert!(ledger
            .clone_created_matching(&child_id, std::ffi::OsStr::new("child"), &child_id)
            .unwrap()
            .is_none());
        ledger
            .clone_created_matching(&parent_id, std::ffi::OsStr::new("child"), &child_id)
            .unwrap()
            .unwrap()
    };
    assert_eq!(
        HandleIdentity::from_directory(&original_clone).unwrap(),
        child_id
    );
    assert_eq!(keeper.retention().unwrap().created, 1);
    keeper.preserve_stale().unwrap();
    let summary = keeper.retention().unwrap();
    assert_eq!(summary.created, 0);
    assert_eq!(summary.stale_evidence, 1);
    assert!(!issuer.plan_creations.lock().unwrap().proves(
        &parent_id,
        &OsString::from("child"),
        &child_id
    ));
    assert!(issuer
        .plan_creations
        .lock()
        .unwrap()
        .clone_created_matching(&parent_id, std::ffi::OsStr::new("child"), &child_id)
        .unwrap()
        .is_none());
    assert_eq!(
        HandleIdentity::from_directory(&original_clone).unwrap(),
        child_id
    );
    keeper.verify_retained().unwrap();
    assert!(fixture.0.join("child").is_dir());
    drop(original_clone);
    drop(parent);
    drop(keeper);
    drop(issuer);
}
