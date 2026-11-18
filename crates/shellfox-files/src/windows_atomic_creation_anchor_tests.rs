use super::*;
use std::{ffi::OsStr, fs};
#[path = "windows_atomic_creation_anchor_fixture.rs"]
mod fixture;
use fixture::*;
#[path = "windows_atomic_creation_anchor_candidate_tests.rs"]
mod candidate_tests;
#[path = "windows_atomic_creation_anchor_failure_tests.rs"]
mod failures;
#[path = "windows_private_frontend_tests.rs"]
mod frontend;
#[test]
fn generic_denial_original_ledger_plan_adoption_and_derived_root_are_distinct() {
    let f = Fixture::new();
    let (authority, plan) = f.plan();
    let mut keeper = authority.directory_preparation_owner();
    let parent = parent(&plan);
    let error = crate::create_private_directory_at(&parent, OsStr::new("generic-denied"))
        .err()
        .unwrap();
    assert!(error
        .error()
        .message
        .contains("untrusted mutating allow ACE"));
    assert!(!f.path.join("parent/generic-denied").exists());
    assert!(!f.path.join("parent/private-child").exists());
    let anchor = WindowsAtomicCreationAnchor::from_plan(&plan).unwrap();
    let mutation = FileService::mutation_lock().unwrap();
    let child = anchor.create(&mutation).unwrap();
    assert_eq!(keeper.retention().unwrap().created, 1);
    keeper.verify_retained().unwrap();
    plan.revalidate().unwrap();
    child.verify().unwrap();
    let root = child.handoff(&plan).unwrap();
    assert!(Arc::ptr_eq(&root.plan_creations, &authority.plan_creations));
    assert!(Arc::ptr_eq(&child.pins.authority, &authority));
    let derived_plan = root.plan_directory(&Location { path: ".".into() }).unwrap();
    assert_eq!(
        derived_plan.original.identities.last(),
        Some(&child.identity)
    );
    drop(derived_plan);
    denied_delete(&parent);
    assert!(fs::rename(f.path.join("parent"), f.path.join("moved-parent")).is_err());
    assert!(fs::rename(
        f.path.join("parent/private-child"),
        f.path.join("parent/rebound")
    )
    .is_err());
    drop(root);
    let evidence = child.release();
    assert_eq!(evidence.leaf, OsStr::new("private-child"));
    assert_eq!(
        evidence.parent,
        HandleIdentity::from_directory(plan.original.dirs.last().unwrap()).unwrap()
    );
    assert_eq!(
        evidence.child,
        HandleIdentity::from_file(
            &crate::private_native::open(
                &parent,
                OsStr::new("private-child"),
                PrivateKind::Directory,
                crate::PrivateAccess::ReadOnly
            )
            .unwrap()
        )
        .unwrap()
    );
    // Dropping local views is NOT release of the ORIGINAL created FD in the bounded keeper.
    denied_delete(&parent);
    drop(mutation);
    keeper.preserve_stale().unwrap();
    assert_eq!(keeper.retention().unwrap().stale_evidence, 1);
    assert!(plan.revalidate().is_err());
    drop(delete_open(&parent).unwrap());
    assert!(f.path.join("parent/private-child").exists());
}
#[test]
fn existing_reopen_and_derived_root_keep_real_independent_pins_until_explicit_release() {
    let f = Fixture::new();
    let (authority, plan) = f.plan();
    let mut keeper = authority.directory_preparation_owner();
    let parent = parent(&plan);
    let mutation = FileService::mutation_lock().unwrap();
    let child = WindowsAtomicCreationAnchor::from_plan(&plan)
        .unwrap()
        .create(&mutation)
        .unwrap();
    let root = child.handoff(&plan).unwrap();
    let existing_plan = authority
        .plan_directory(&Location {
            path: "parent/private-child".into(),
        })
        .unwrap();
    let existing = PinnedPrivateDirectory::open_existing(&existing_plan, &mutation).unwrap();
    assert_eq!(existing.identity, child.identity);
    assert_eq!(keeper.retention().unwrap().created, 1);
    existing.verify().unwrap();
    let reopened_root = existing.handoff(&existing_plan).unwrap();
    assert!(Arc::ptr_eq(
        &reopened_root.plan_creations,
        &authority.plan_creations
    ));
    let generic = crate::open_existing_private_at(
        &parent,
        OsStr::new("private-child"),
        PrivateKind::Directory,
        crate::PrivateAccess::ReadOnly,
    )
    .err()
    .unwrap();
    assert!(generic.message.contains("untrusted mutating allow ACE"));
    drop(mutation);
    keeper.preserve_stale().unwrap();
    assert!(plan.revalidate().is_err());
    denied_delete(&parent);
    child.release();
    existing.release();
    drop(existing_plan);
    drop(root);
    denied_delete(&parent);
    drop(reopened_root);
    drop(delete_open(&parent).unwrap());
    fs::rename(
        f.path.join("parent/private-child"),
        f.path.join("parent/rebound"),
    )
    .unwrap();
    assert!(f.path.join("parent/rebound").exists());
}
#[test]
fn existing_current_state_verification_never_creates_a_birth_or_new_ledger_row() {
    let f = Fixture::new();
    let (authority, plan) = f.plan();
    let mut keeper = authority.directory_preparation_owner();
    let parent = parent(&plan);
    let mutation = FileService::mutation_lock().unwrap();
    let child = WindowsAtomicCreationAnchor::from_plan(&plan)
        .unwrap()
        .create(&mutation)
        .unwrap();
    let identity = child.identity.clone();
    child.release();
    drop(mutation);
    keeper.preserve_stale().unwrap();
    assert_eq!(keeper.retention().unwrap().created, 0);
    drop(delete_open(&parent).unwrap());
    let existing_plan = authority
        .plan_directory(&Location {
            path: "parent/private-child".into(),
        })
        .unwrap();
    let mutation = FileService::mutation_lock().unwrap();
    let existing = PinnedPrivateDirectory::open_existing(&existing_plan, &mutation).unwrap();
    assert_eq!(existing.identity, identity);
    assert_eq!(keeper.retention().unwrap().created, 0);
    assert_eq!(keeper.retention().unwrap().stale_evidence, 1);
    drop(existing_plan);
    denied_delete(&parent);
    existing.release();
    drop(delete_open(&parent).unwrap());
    assert!(f.path.join("parent/private-child").exists());
}
#[test]
fn current_privacy_loss_blocks_verify_reopen_and_handoff_without_repair() {
    let f = Fixture::new();
    let (authority, plan) = f.plan();
    let mut keeper = authority.directory_preparation_owner();
    let mutation = FileService::mutation_lock().unwrap();
    let child = WindowsAtomicCreationAnchor::from_plan(&plan)
        .unwrap()
        .create(&mutation)
        .unwrap();
    let path = f.path.join("parent/private-child");
    crate::private_test_windows::unprotect(&path);
    let before = crate::private_test_windows::security(&path);
    assert!(child.verify().is_err());
    assert!(child.handoff(&plan).is_err());
    let existing_plan = authority
        .plan_directory(&Location {
            path: "parent/private-child".into(),
        })
        .unwrap();
    assert!(PinnedPrivateDirectory::open_existing(&existing_plan, &mutation).is_err());
    assert_eq!(crate::private_test_windows::security(&path), before);
    assert_eq!(keeper.retention().unwrap().created, 1);
    child.release();
    drop(existing_plan);
    drop(mutation);
    keeper.preserve_stale().unwrap();
}
#[test]
fn original_revocation_fences_existing_reopen_and_already_derived_root() {
    let f = Fixture::new();
    let (authority, plan) = f.plan();
    let keeper = authority.directory_preparation_owner();
    let mutation = FileService::mutation_lock().unwrap();
    let child = WindowsAtomicCreationAnchor::from_plan(&plan)
        .unwrap()
        .create(&mutation)
        .unwrap();
    let root = child.handoff(&plan).unwrap();
    let existing_plan = authority
        .plan_directory(&Location {
            path: "parent/private-child".into(),
        })
        .unwrap();
    drop(mutation);
    authority.revoke().unwrap();
    assert!(child.verify().is_err());
    assert!(root.stat(&Location { path: ".".into() }).is_err());
    let mutation = FileService::mutation_lock().unwrap();
    assert!(PinnedPrivateDirectory::open_existing(&existing_plan, &mutation).is_err());
    keeper.verify_retained().unwrap();
    child.release();
}
#[test]
fn exclusive_ordinary_and_reparse_collisions_never_adopt_or_repair() {
    for reparse in [false, true] {
        let f = Fixture::new();
        let (authority, plan) = f.plan();
        let mut keeper = authority.directory_preparation_owner();
        let anchor = WindowsAtomicCreationAnchor::from_plan(&plan).unwrap();
        if reparse {
            fs::create_dir(f.path.join("outside")).unwrap();
            crate::private_test_junction::create(
                &f.path.join("outside"),
                &f.path.join("parent/private-child"),
            );
        } else {
            fs::create_dir(f.path.join("parent/private-child")).unwrap();
        }
        let before = crate::private_test_windows::security(&f.path.join("parent/private-child"));
        let mutation = FileService::mutation_lock().unwrap();
        let failure = anchor.create(&mutation).err().unwrap();
        assert_eq!(failure.retention().unwrap().created, 0);
        failure.verify_retained().unwrap();
        assert_eq!(
            crate::private_test_windows::security(&f.path.join("parent/private-child")),
            before
        );
        drop(mutation);
        keeper.preserve_stale().unwrap();
        assert!(f.path.join("parent/private-child").exists());
    }
}
