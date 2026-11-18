//! Injected owner-boundary test, not private birth/kernel/VFS acceptance.
use super::*;
use cap_std::ambient_authority;
use std::{
    fs,
    panic::{catch_unwind, AssertUnwindSafe},
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
        let path = base.join(format!("directory-owner-panic-{}", uuid::Uuid::new_v4()));
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
fn keeper_preserves_pre_registered_possible_artifact_after_ledger_panic() {
    let fixture = Fixture::new();
    let issuer =
        Arc::new(FileService::approved_root(&fixture.0, crate::Access::ReadWrite).unwrap());
    let mut keeper = issuer.directory_preparation_owner();
    // The real original issuer, keeper and all five plans are OUTSIDE the catch.
    let plans: Vec<_> = ["ui", "device", "run", "editor", "host"]
        .into_iter()
        .map(|name| {
            issuer
                .plan_directory(&crate::Location {
                    path: fixture.0.join(name).to_str().unwrap().into(),
                })
                .unwrap()
        })
        .collect();
    let original = Dir::open_ambient_dir(&fixture.0, ambient_authority()).unwrap();
    let identity = HandleIdentity::from_directory(&original).unwrap();
    // Inspection works while normal mutation is held; terminal preservation
    // releases ledger then takes mutation, so drop this guard before preserving.
    // Do not poison this process-global lock and affect unrelated leaf tests.
    let mutation = FileService::mutation_lock().unwrap();
    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut ledger = issuer.plan_creations.lock().unwrap();
        ledger.reserve(1).unwrap();
        ledger.begin(original, identity, OsString::from("ui"));
        // Boundary before an actual returned child File can be stored. The row
        // must survive as POSSIBLE artifact evidence, never as NoCreate.
        panic!("injected creation boundary");
    }));
    assert!(result.is_err());
    assert!(issuer.plan_creations.lock().is_err());
    let summary = keeper.retention().unwrap();
    assert_eq!(summary.uncertain, 1);
    assert_eq!(summary.created, 0);
    keeper.verify_retained().unwrap();
    assert!(!fixture.0.join("ui").exists());
    drop(mutation);
    assert_eq!(
        keeper.preserve_stale().unwrap(),
        PrivatePreservationState::StaleArtifactEvidence
    );
    let summary = keeper.retention().unwrap();
    assert_eq!(summary.uncertain, 0);
    assert_eq!(summary.stale_evidence, 1);
    // Conservative preservation did not recover normal mutation authority.
    assert!(issuer.plan_creations.lock().is_err());
    assert!(plans[0].create_private_approved_root().is_err());
    assert!(plans[1].create_private_approved_root().is_err());
    keeper.verify_retained().unwrap();
    drop(plans);
    drop(keeper);
    drop(issuer);
}
