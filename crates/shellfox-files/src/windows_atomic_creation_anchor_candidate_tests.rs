use super::*;
use crate::directory_plan::create::DirectoryCreatorOrigin;
#[test]
fn candidate_multi_missing_and_current_private_prefix_use_original_records() {
    let f = Fixture::new();
    let (authority, first) = f.plan();
    let keeper = authority.directory_preparation_owner();
    let nested = authority
        .plan_directory(&Location {
            path: "parent/private-child/next/final".into(),
        })
        .unwrap();
    let root = first.create_private_approved_root().unwrap();
    let final_root = nested.create_private_approved_root().unwrap();
    assert_eq!(keeper.retention().unwrap().created, 3);
    keeper.verify_retained().unwrap();
    first.revalidate().unwrap();
    nested.revalidate().unwrap();
    assert!(Arc::ptr_eq(
        &authority.plan_creations,
        &final_root.plan_creations
    ));
    let current = nested.current().unwrap();
    let ledger = authority.plan_creations.lock().unwrap();
    for i in nested.original.dirs.len()..current.dirs.len() {
        assert_eq!(
            ledger.created_origin(
                &current.identities[i - 1],
                &current.names[i - 1],
                &current.identities[i]
            ),
            Some(DirectoryCreatorOrigin::PrivateCreator)
        );
    }
    drop(ledger);
    assert!(root
        .stat(&Location {
            path: "next/final".into()
        })
        .is_ok());
}
#[test]
fn candidate_existing_reopen_has_no_created_row_or_creation_effect() {
    let f = Fixture::new();
    let (authority, missing) = f.plan();
    let keeper = authority.directory_preparation_owner();
    let root = missing.create_private_approved_root().unwrap();
    let existing = authority
        .plan_directory(&Location {
            path: "parent/private-child".into(),
        })
        .unwrap();
    let reopened = existing.create_private_approved_root().unwrap();
    assert_eq!(keeper.retention().unwrap().created, 1);
    assert!(Arc::ptr_eq(&root.plan_creations, &reopened.plan_creations));
    crate::private_test_windows::unprotect(&f.path.join("parent/private-child"));
    assert!(existing.create_private_approved_root().is_err());
    // Current-state read/reopen rejection with no create does not invent a creation effect.
    let admission = authority.acquire_directory_admission().unwrap();
    admission.finish(Ok(())).unwrap();
}
#[test]
fn candidate_refuses_foreign_or_ordinary_private_looking_prefix_without_retag() {
    for ordinary in [false, true] {
        let f = Fixture::new();
        let (authority, _first) = f.plan();
        let nested = authority
            .plan_directory(&Location {
                path: "parent/private-child/next".into(),
            })
            .unwrap();
        if ordinary {
            let first = authority
                .plan_directory(&Location {
                    path: "parent/private-child".into(),
                })
                .unwrap();
            let root = first.create_approved_root().unwrap();
            // Owned fixture deliberately changes CURRENT security, never historical creator origin.
            let current = first.current().unwrap();
            crate::upload_private::protect_retained_directory(current.dirs.last().unwrap())
                .unwrap();
            drop(root);
            let current = first.current().unwrap();
            let last = current.identities.len() - 1;
            assert_eq!(
                authority.plan_creations.lock().unwrap().created_origin(
                    &current.identities[last - 1],
                    current.names.last().unwrap(),
                    &current.identities[last]
                ),
                Some(DirectoryCreatorOrigin::Ordinary)
            );
        } else {
            fs::create_dir(f.path.join("parent/private-child")).unwrap();
        }
        assert!(nested.create_private_approved_root().is_err());
        assert!(!f.path.join("parent/private-child/next").exists());
    }
}
#[test]
fn candidate_posteffect_error_or_unwind_quarantines_and_preserves_without_revoke() {
    for unwind in [false, true] {
        let f = Fixture::new();
        let (authority, _first) = f.plan();
        let mut keeper = authority.directory_preparation_owner();
        fs::write(f.path.join("user.txt"), b"normal-user-io").unwrap();
        let nested = authority
            .plan_directory(&Location {
                path: "parent/private-child/next/final".into(),
            })
            .unwrap();
        let failure = candidate::prepare_with(&nested, |step| {
            if step == 2 {
                if unwind {
                    panic!("candidate interrupted after real capture");
                }
                return Err(FileError::new(
                    crate::ErrorCode::StorageFailed,
                    "candidate post-create failure",
                ));
            }
            Ok(())
        })
        .err()
        .unwrap();
        assert_eq!(failure.retention().unwrap().created, 2);
        failure.verify_retained().unwrap();
        assert!(authority.acquire_directory_admission().is_err());
        assert!(nested.create_private_approved_root().is_err());
        assert!(!f.path.join("parent/private-child/next/final").exists());
        assert_eq!(
            authority
                .read_text(&Location {
                    path: "user.txt".into()
                })
                .unwrap()
                .text,
            "normal-user-io"
        );
        drop(FileService::mutation_lock().unwrap());
        keeper.preserve_stale().unwrap();
        assert!(authority.acquire_directory_admission().is_err());
        assert_eq!(
            authority
                .read_text(&Location {
                    path: "user.txt".into()
                })
                .unwrap()
                .text,
            "normal-user-io"
        );
    }
}
#[test]
fn candidate_active_preserve_and_second_admission_are_busy_without_closing_pins() {
    let f = Fixture::new();
    let (authority, plan) = f.plan();
    let mut keeper = authority.directory_preparation_owner();
    let parent = parent(&plan);
    let root = candidate::prepare_with(&plan, |_| {
        assert!(keeper.preserve_stale().is_err());
        assert!(authority.acquire_directory_admission().is_err());
        denied_delete(&parent);
        Ok(())
    })
    .unwrap();
    keeper.preserve_stale().unwrap();
    assert!(plan.create_private_approved_root().is_err());
    denied_delete(&parent);
    drop(root);
    drop(delete_open(&parent).unwrap());
    assert!(f.path.join("parent/private-child").exists());
}
#[test]
fn candidate_current_privacy_failure_after_capture_keeps_private_creator_fact_not_a_receipt() {
    let f = Fixture::new();
    let (authority, plan) = f.plan();
    let mut keeper = authority.directory_preparation_owner();
    let path = f.path.join("parent/private-child");
    let mut changed = None;
    let failure = candidate::prepare_with(&plan, |_| {
        crate::private_test_windows::unprotect(&path);
        changed = Some(crate::private_test_windows::security(&path));
        Ok(())
    })
    .err()
    .unwrap();
    assert_eq!(failure.retention().unwrap().created, 1);
    assert_eq!(
        crate::private_test_windows::security(&path),
        changed.unwrap()
    );
    assert!(authority.acquire_directory_admission().is_err());
    keeper.verify_retained().unwrap();
    keeper.preserve_stale().unwrap();
}
