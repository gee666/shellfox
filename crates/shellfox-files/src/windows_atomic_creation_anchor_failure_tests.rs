use super::*;
use std::panic::{catch_unwind, AssertUnwindSafe};
#[test]
fn revoked_original_issuer_cancels_before_reservation_or_native_mutation() {
    let f = Fixture::new();
    let (authority, plan) = f.plan();
    let keeper = authority.directory_preparation_owner();
    let anchor = WindowsAtomicCreationAnchor::from_plan(&plan).unwrap();
    authority.revoke().unwrap();
    let mutation = FileService::mutation_lock().unwrap();
    let failure = anchor
        .create_with(&mutation, || {
            panic!("cancelled issuer reached a post-create hook")
        })
        .err()
        .unwrap();
    assert_eq!(failure.error().code, crate::ErrorCode::AccessDenied);
    assert_eq!(keeper.retention().unwrap().created, 0);
    assert_eq!(keeper.retention().unwrap().live_custody, 0);
    assert!(!f.path.join("parent/private-child").exists());
}
#[test]
fn post_registration_cancel_retains_actual_native_fd_in_original_owner() {
    let f = Fixture::new();
    let (authority, plan) = f.plan();
    let mut keeper = authority.directory_preparation_owner();
    let parent = parent(&plan);
    let token = crate::EditorCancellation::default();
    let anchor = WindowsAtomicCreationAnchor::from_plan(&plan).unwrap();
    let mutation = FileService::mutation_lock().unwrap();
    let failure = anchor
        .create_with(&mutation, || {
            assert_eq!(keeper.retention().unwrap().created, 1);
            denied_delete(&parent);
            token.cancel();
            assert!(token.is_cancelled());
            Err(FileError::new(
                crate::ErrorCode::AccessDenied,
                "Injected post-registration cancellation.",
            ))
        })
        .err()
        .unwrap();
    assert_eq!(failure.error().code, crate::ErrorCode::AccessDenied);
    failure.verify_retained().unwrap();
    plan.revalidate().unwrap();
    denied_delete(&parent);
    assert!(f.path.join("parent/private-child").is_dir());
    drop(mutation);
    keeper.preserve_stale().unwrap();
    drop(delete_open(&parent).unwrap());
    assert!(f.path.join("parent/private-child").is_dir());
}
#[test]
fn post_registration_faults_and_privacy_rejection_keep_created_custody_without_repair() {
    for fault in ["diagnostic", "privacy"] {
        let f = Fixture::new();
        let (authority, plan) = f.plan();
        let mut keeper = authority.directory_preparation_owner();
        let parent = parent(&plan);
        let path = f.path.join("parent/private-child");
        let mut changed = None;
        let mutation = FileService::mutation_lock().unwrap();
        let anchor = WindowsAtomicCreationAnchor::from_plan(&plan).unwrap();
        let failure = anchor
            .create_with(&mutation, || {
                assert_eq!(keeper.retention().unwrap().created, 1);
                denied_delete(&parent);
                if fault == "diagnostic" {
                    return Err(FileError::new(
                        crate::ErrorCode::StorageFailed,
                        "Injected post-registration validation fault.",
                    ));
                }
                crate::private_test_windows::unprotect(&path);
                changed = Some(crate::private_test_windows::security(&path));
                Ok(())
            })
            .err()
            .unwrap();
        assert_eq!(failure.retention().unwrap().created, 1);
        failure.verify_retained().unwrap();
        denied_delete(&parent);
        if let Some(before) = changed {
            assert_eq!(crate::private_test_windows::security(&path), before);
        }
        drop(mutation);
        keeper.preserve_stale().unwrap();
        drop(delete_open(&parent).unwrap());
        assert!(path.is_dir());
    }
}
#[test]
fn unwind_after_actual_fd_registration_survives_in_external_keeper_without_mutex_poison() {
    let f = Fixture::new();
    let (authority, plan) = f.plan();
    let mut keeper = authority.directory_preparation_owner();
    let weak = Arc::downgrade(&authority);
    let parent = parent(&plan);
    let anchor = WindowsAtomicCreationAnchor::from_plan(&plan).unwrap();
    let mutation = FileService::mutation_lock().unwrap();
    // Real original owner, plan and MUTATIONS guard are all OUTSIDE catch_unwind.
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        anchor.create_with(&mutation, || {
            assert_eq!(keeper.retention().unwrap().created, 1);
            denied_delete(&parent);
            panic!("injected after original ledger owns actual native FD");
        })
    }));
    assert!(outcome.is_err());
    assert!(matches!(
        crate::service::MUTATIONS.try_lock(),
        Err(std::sync::TryLockError::WouldBlock)
    ));
    assert!(authority.plan_creations.lock().is_ok());
    assert_eq!(keeper.retention().unwrap().created, 1);
    keeper.verify_retained().unwrap();
    plan.revalidate().unwrap();
    drop(mutation);
    drop(FileService::mutation_lock().unwrap());
    drop(plan);
    drop(authority);
    assert!(weak.upgrade().is_some());
    denied_delete(&parent);
    keeper.preserve_stale().unwrap();
    assert_eq!(keeper.retention().unwrap().stale_evidence, 1);
    drop(delete_open(&parent).unwrap());
    assert!(f.path.join("parent/private-child").is_dir());
    drop(keeper);
    assert!(weak.upgrade().is_none());
}
