//! Admission/effect mechanics only. No Windows proof, creation/privacy or VFS acceptance.
use super::*;
use crate::{Access, PrivatePreservationState};
fn issuer() -> Arc<FileService> {
    Arc::new(FileService::local_desktop(Access::ReadWrite))
}
fn phase(issuer: &Arc<FileService>) -> DirectoryPhase {
    issuer
        .plan_creations
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .directory_phase()
}
#[test]
fn active_preservation_is_busy_before_mutation_wait_and_finish_is_same_domain() {
    let issuer = issuer();
    let mut keeper = issuer.directory_preparation_owner();
    let clone = keeper.clone();
    let guard = issuer.acquire_directory_admission().unwrap();
    guard.check_issuer(&issuer).unwrap();
    assert!(guard.check_issuer(&self::issuer()).is_err());
    assert!(crate::service::MUTATIONS.try_lock().is_err());
    assert!(issuer.acquire_directory_admission().is_err());
    // Same thread holds the actual global guard: a MUTATIONS wait here would deadlock.
    assert!(keeper.preserve_stale().is_err());
    assert_eq!(phase(&issuer), DirectoryPhase::Active);
    assert_eq!(clone.retention().unwrap().created, 0);
    let _: &MutexGuard<'static, ()> = guard.mutation();
    guard.finish::<()>(Ok(())).unwrap();
    assert_eq!(phase(&issuer), DirectoryPhase::Idle);
    let next = issuer.acquire_directory_admission().unwrap();
    next.finish::<()>(Ok(())).unwrap();
    keeper.preserve_stale().unwrap();
    assert_eq!(phase(&issuer), DirectoryPhase::Preserved);
    assert!(issuer.acquire_directory_admission().is_err());
    // Admission preservation is not global FileService revocation.
    issuer.authorized(true).unwrap();
}
#[test]
fn no_rows_do_not_erase_possible_effect_and_drop_does_not_rearm() {
    let issuer = issuer();
    let mut keeper = issuer.directory_preparation_owner();
    let guard = issuer.acquire_directory_admission().unwrap();
    let error = DirectoryPreparationFailure::new(FileError::denied(), issuer.clone());
    assert!(guard.finish::<()>(Err(error)).is_err());
    assert_eq!(phase(&issuer), DirectoryPhase::Idle);
    let mut guard = issuer.acquire_directory_admission().unwrap();
    guard.mark_effect_possible();
    assert_eq!(keeper.retention().unwrap().created, 0);
    let error = DirectoryPreparationFailure::new(FileError::denied(), issuer.clone());
    assert!(guard.finish::<()>(Err(error)).is_err());
    assert_eq!(phase(&issuer), DirectoryPhase::Faulted);
    assert!(issuer.acquire_directory_admission().is_err());
    assert_eq!(
        keeper.preserve_stale().unwrap(),
        PrivatePreservationState::StaleArtifactEvidence
    );
    assert_eq!(phase(&issuer), DirectoryPhase::Preserved);
    issuer.authorized(true).unwrap();
    let other = self::issuer();
    let _keeper = other.directory_preparation_owner();
    drop(other.acquire_directory_admission().unwrap());
    assert_eq!(phase(&other), DirectoryPhase::Faulted);
}
#[test]
fn retained_guard_outside_unwind_can_record_interruption_without_global_poison() {
    let issuer = issuer();
    let keeper = issuer.directory_preparation_owner();
    let mut guard = issuer.acquire_directory_admission().unwrap();
    guard.mark_effect_possible();
    let caught = std::panic::catch_unwind(|| panic!("injected owned operation boundary"));
    assert!(caught.is_err());
    let error = DirectoryPreparationFailure::new(
        FileError::new(
            crate::ErrorCode::StorageFailed,
            "Interrupted directory operation.",
        ),
        issuer.clone(),
    );
    assert!(guard.finish::<()>(Err(error)).is_err());
    assert_eq!(phase(&issuer), DirectoryPhase::Faulted);
    keeper.verify_retained().unwrap();
    assert!(!crate::service::MUTATIONS.is_poisoned());
}
