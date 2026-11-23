use super::*;
use sha2::{Digest, Sha256};
use std::panic::{catch_unwind, AssertUnwindSafe};

fn helper() -> VerifiedHelper {
    // Pure bookkeeping tests never execute this verified, non-executable file.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let hash = Sha256::digest(std::fs::read(&path).unwrap()).into();
    VerifiedHelper::new(path, hash).unwrap()
}
fn control(scope: &OfflineScope) -> Arc<OfflineControl> {
    OfflineControl::prepare(scope.0.helper.clone(), None, scope.root_lifetime())
}

#[tokio::test]
async fn clean_empty_scope_requires_seal_and_produces_transport_proof() {
    let scope = OfflineScope::new(helper(), false).unwrap();
    scope.cleanup_confirmed().await.unwrap();
    assert!(matches!(scope.forward(), Err(HelperError::Stopped)));
    scope.cleanup_confirmed().await.unwrap();
}

#[tokio::test]
async fn entered_without_original_is_unknown_not_proven_no_child() {
    let scope = OfflineScope::new(helper(), false).unwrap();
    let original = control(&scope);
    original.enter_spawn().unwrap();
    scope.forward().unwrap().children.push(original.clone());
    assert_eq!(original.cancel_before_entry(), Err(HelperError::Stopped));
    assert!(!original.resolved_cached());
    let result = tokio::time::timeout(Duration::from_millis(1), scope.cleanup_confirmed()).await;
    assert!(result.is_err());
    assert_eq!(scope.0.ledger.lock().unwrap().children.len(), 1);
    assert!(!original.resolved_cached());
}

#[tokio::test]
async fn typed_pre_entry_no_child_resolves_without_fabricating_exit_status() {
    let scope = OfflineScope::new(helper(), false).unwrap();
    let original = control(&scope);
    scope.forward().unwrap().children.push(original.clone());
    original.cancel_before_entry().unwrap();
    assert!(original.resolved_cached());
    assert_eq!(original.poll_exit().unwrap(), None);
    scope.cleanup_confirmed().await.unwrap();
}

#[tokio::test]
async fn generic_spawn_error_keeps_entered_uncertainty_and_artifact() {
    let scope = OfflineScope::new(helper(), false).unwrap();
    // The verified Cargo.toml is not executable. Even this OS spawn error is
    // not promoted to ProvenNoChild by the producer.
    assert!(matches!(scope.spawn(&[], false), Err(HelperError::Spawn)));
    {
        let ledger = scope.forward().unwrap();
        assert_eq!(ledger.children.len(), 1);
        assert!(!ledger.children[0].resolved_cached());
        assert_eq!(
            ledger.children[0].cancel_before_entry(),
            Err(HelperError::Stopped)
        );
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(1), scope.cleanup_confirmed(),)
            .await
            .is_err()
    );
}

#[test]
fn thirty_two_uncertain_slots_are_not_pruned_or_replaced_by_new_admission() {
    let scope = OfflineScope::new(helper(), false).unwrap();
    for _ in 0..MAX_PENDING {
        let original = control(&scope);
        original.enter_spawn().unwrap();
        scope.forward().unwrap().children.push(original);
    }
    // Returns before OS spawn because all 32 original slots remain unresolved.
    assert!(matches!(scope.spawn(&[], false), Err(HelperError::Busy)));
    let ledger = scope.forward().unwrap();
    assert_eq!(ledger.children.len(), MAX_PENDING);
    assert!(ledger.children.iter().all(|child| !child.resolved_cached()));
}

#[tokio::test]
async fn poison_denies_forward_and_does_not_become_clean_empty_registry() {
    let scope = OfflineScope::new(helper(), false).unwrap();
    assert!(catch_unwind(AssertUnwindSafe(|| {
        let _guard = scope.forward().unwrap();
        panic!("interrupted admission");
    }))
    .is_err());
    assert!(matches!(scope.forward(), Err(HelperError::Stopped)));
    assert!(matches!(
        scope.cleanup_confirmed().await,
        Err(HelperError::Stopped)
    ));
    assert!(scope.0.ledger.is_poisoned());
}

#[tokio::test]
async fn cleanup_view_keeps_original_artifact_and_root_across_compatibility_moves() {
    let lease = Arc::new(());
    let weak = Arc::downgrade(&lease);
    let mut preparation =
        crate::OfflinePreparation::new(helper().with_resource_guard(lease)).unwrap();
    let cleanup = preparation.original_cleanup_view();
    let same_scope = preparation.scope.clone();
    let root =
        BoundOwnershipRoot::capture_current(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))
            .unwrap();
    preparation
        .retain_original_directories(root.original_files().unwrap())
        .unwrap();
    let identity = root.identity().clone();
    let attempt = preparation.bind_root(root).unwrap();
    assert_eq!(
        same_scope
            .0
            .ledger
            .lock()
            .unwrap()
            .root
            .as_ref()
            .unwrap()
            .identity(),
        &identity,
    );
    drop(attempt);
    assert!(weak.upgrade().is_some());
    cleanup.shutdown_confirmed().await.unwrap();
    drop(same_scope);
    drop(cleanup);
    assert!(weak.upgrade().is_none());
}

#[test]
fn cached_control_recheck_does_not_resolve_or_release_unknown_admission() {
    let scope = OfflineScope::new(helper(), false).unwrap();
    let original = control(&scope);
    original.enter_spawn().unwrap();
    for _ in 0..3 {
        assert_eq!(
            original.check_cached_owner(),
            Err(HelperError::OwnershipRequired)
        );
    }
    assert!(!original.lost().is_cancelled());
    assert!(!original.resolved_cached());
    assert_eq!(original.cancel_before_entry(), Err(HelperError::Stopped));
}

// Compilation alone verifies the guard cannot be sent to another thread.
const _: fn() = || {
    trait AmbiguousIfSend<A> {
        fn marker() {}
    }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    struct IsSend;
    impl<T: ?Sized + Send> AmbiguousIfSend<IsSend> for T {}
    let _ = <crate::NativeAdmission<'static> as AmbiguousIfSend<_>>::marker;
};
