#![cfg(feature = "test-helper")]
use shellfox_transport::*;
use std::{
    fs::File,
    path::Path,
    sync::{mpsc, Arc},
    time::Duration,
};
mod support;
use support::{executable, Scratch};

fn original_chain(path: &Path) -> (BoundOwnershipRoot, Vec<File>) {
    let path = normalize_private_path(&std::fs::canonicalize(path).unwrap()).unwrap();
    let mut ancestors: Vec<_> = path.ancestors().collect();
    ancestors.reverse();
    let files: Vec<_> = ancestors
        .into_iter()
        .map(|directory| {
            let mut options = std::fs::OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.share_mode(3).custom_flags(0x02000000 | 0x00200000);
            }
            options.open(directory).unwrap()
        })
        .collect();
    let root =
        BoundOwnershipRoot::from_held(path, files.last().unwrap().try_clone().unwrap()).unwrap();
    (root, files)
}
fn preparation(path: &Path) -> OfflinePreparation {
    let mut preparation = OfflinePreparation::new(executable()).unwrap();
    let (root, files) = original_chain(path);
    preparation.retain_original_directories(files).unwrap();
    preparation.bind_root_borrowed(&root).unwrap();
    preparation
}

#[tokio::test]
async fn same_original_fence_drains_before_seal_and_denies_every_later_entry() {
    let scratch = Scratch::new();
    let mut preparation = preparation(&scratch.0);
    let cleanup = preparation.original_cleanup_view();
    assert!(matches!(
        preparation.acquire_borrowed().await.unwrap(),
        OwnershipDecision::Acquired
    ));
    let acquired = preparation.acquired_view().unwrap();
    let lost = acquired.lost();
    let lifetime = acquired.root_lifetime();
    let fence = acquired.native_fence();
    let another_fence = acquired.native_fence();
    let admission = fence.enter().unwrap();
    admission.check_original_owner().unwrap();
    let (started, waiting) = mpsc::channel();
    let (finished, sealed) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        started.send(()).unwrap();
        cleanup.seal();
        finished.send(()).unwrap();
        cleanup
    });
    waiting.recv().unwrap();
    assert!(sealed.recv_timeout(Duration::from_millis(20)).is_err());
    admission.check_original_owner().unwrap();
    drop(admission); // No await while the synchronous native permit is live.
    sealed.recv_timeout(Duration::from_secs(2)).unwrap();
    let cleanup = thread.join().unwrap();
    assert!(fence.enter().is_err());
    assert!(another_fence.enter().is_err());
    assert!(preparation.acquired_view().is_err());
    // These retained observations cannot reauthorize a sealed scope.
    assert!(!lost.is_cancelled());
    drop(lifetime);
    cleanup.shutdown_confirmed().await.unwrap();
    assert!(lost.is_cancelled());
    cleanup.shutdown_confirmed().await.unwrap();
}

#[tokio::test]
async fn phase_moves_and_drop_keep_original_cleanup_and_artifact_lease() {
    let scratch = Scratch::new();
    let lease = Arc::new(());
    let weak = Arc::downgrade(&lease);
    let mut preparation = OfflinePreparation::new(executable().with_resource_guard(lease)).unwrap();
    let cleanup = preparation.original_cleanup_view();
    let (root, files) = original_chain(&scratch.0);
    preparation.retain_original_directories(files).unwrap();
    let mut attempt = preparation.bind_root(root).unwrap();
    assert!(matches!(
        attempt.acquire().await.unwrap(),
        OwnershipDecision::Acquired
    ));
    let owner = attempt.into_acquired().unwrap();
    drop(owner);
    assert!(weak.upgrade().is_some());
    cleanup.shutdown_confirmed().await.unwrap();
    assert!(weak.upgrade().is_some());
    drop(cleanup);
    // The existing monitor may still be completing its own confirmed retirement.
    tokio::time::timeout(Duration::from_secs(2), async {
        while weak.upgrade().is_some() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn scoped_protection_requires_actual_owner_and_cannot_bypass_seal() {
    let scratch = Scratch::new();
    let mut preparation = preparation(&scratch.0);
    let cleanup = preparation.original_cleanup_view();
    assert_eq!(
        preparation
            .permission_verifier()
            .protect(&scratch.0, PrivatePathKind::Directory)
            .await,
        Err(HelperError::OwnershipRequired),
    );
    assert!(matches!(
        preparation.acquire_borrowed().await.unwrap(),
        OwnershipDecision::Acquired
    ));
    // Separate registered admission: no NativeAdmission/Files/slot guard here.
    preparation
        .permission_verifier()
        .protect(&scratch.0, PrivatePathKind::Directory)
        .await
        .unwrap();
    cleanup.seal();
    assert!(preparation
        .permission_verifier()
        .protect(&scratch.0, PrivatePathKind::Directory)
        .await
        .is_err());
    cleanup.shutdown_confirmed().await.unwrap();
}

#[tokio::test]
async fn failed_binding_keeps_original_root_and_preparation_for_cleanup() {
    let scratch = Scratch::new();
    let mut preparation = OfflinePreparation::new(executable()).unwrap();
    let cleanup = preparation.original_cleanup_view();
    let (root, files) = original_chain(&scratch.0);
    assert_eq!(
        preparation.bind_root_borrowed(&root),
        Err(HelperError::RootChainRequired)
    );
    preparation.retain_original_directories(files).unwrap();
    preparation.bind_root_borrowed(&root).unwrap();
    assert_eq!(
        preparation.bind_root_borrowed(&root),
        Err(HelperError::Stopped)
    );
    drop(root);
    cleanup.shutdown_confirmed().await.unwrap();
    assert!(preparation.acquired_view().is_err());
}

#[tokio::test]
async fn cancelled_borrowed_acquisition_does_not_replay_or_lose_cleanup() {
    use std::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    let scratch = Scratch::new();
    let mut preparation = preparation(&scratch.0);
    let cleanup = preparation.original_cleanup_view();
    let mut start = Box::pin(preparation.acquire_borrowed());
    let mut context = Context::from_waker(Waker::noop());
    match start.as_mut().poll(&mut context) {
        Poll::Pending => {}
        Poll::Ready(result) => {
            result.unwrap();
        }
    }
    drop(start);
    assert!(matches!(
        preparation.acquire_borrowed().await,
        Err(HelperError::Stopped)
    ));
    cleanup.shutdown_confirmed().await.unwrap();
    assert!(preparation.acquired_view().is_err());
}
