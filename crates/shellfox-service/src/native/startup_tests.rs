//! Bootstrap ownership proof, including errors before the public endpoint exists.
use crate::local::{serve_with_boot_status, LocalBootStatus};
use crate::tests::scratch;
use std::sync::{Arc, Mutex};

#[tokio::test(flavor = "current_thread")]
async fn preownership_validation_rejection_proves_clean_without_ready_or_stop_rpc() {
    let data = scratch();
    std::fs::write(
        data.path().join("not-an-experiment-artifact"),
        b"preserve fixture",
    )
    .unwrap();
    let statuses = Arc::new(Mutex::new(Vec::new()));
    let recorded = statuses.clone();
    let ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ready_callback = ready.clone();
    assert!(serve_with_boot_status(
        data.path(),
        &data.path().join("missing-helper.exe"),
        [0; 32],
        (),
        move |_| {
            ready_callback.store(true, std::sync::atomic::Ordering::SeqCst);
        },
        move |status| recorded.lock().unwrap().push(status)
    )
    .await
    .is_err());
    assert_eq!(
        statuses.lock().unwrap().last(),
        Some(&LocalBootStatus::NoOwnerClean)
    );
    assert!(!ready.load(std::sync::atomic::Ordering::SeqCst));
    assert!(!data.path().join(shellfox_host::STORE_FILENAME).exists());
}

#[tokio::test(flavor = "current_thread")]
async fn failure_after_go_ownership_joins_actor_releases_helper_and_allows_lock_reacquire() {
    use shellfox_transport::{HostOwnership, VerifiedHelper};
    let scratch = scratch();
    let data = scratch.path().join("data");
    let (helper, digest) = crate::tests::helper();
    let mut boot = crate::local::LocalBootstrap::plan(&data, &helper, digest).unwrap();
    let stop = boot.stop_handle();
    boot.run_with_dispatcher((), move |_| stop.request(), |_| {})
        .await
        .unwrap();
    drop(boot);
    // Unix can preserve a stopped endpoint artifact. Failed startup must not replace it.
    let endpoint = data.join("rust-service-v1/endpoint.json");
    let stopped_endpoint = std::fs::read(&endpoint).ok();
    // Corrupt the existing protected store so the fixture reaches SQLite.
    let corrupt = data.join(shellfox_host::STORE_FILENAME);
    std::fs::write(&corrupt, b"deliberately not a sqlite database").unwrap();
    let statuses = Arc::new(Mutex::new(Vec::new()));
    let recorded = statuses.clone();
    let result = serve_with_boot_status(
        &data,
        &helper,
        digest,
        (),
        |_| panic!("failed boot cannot publish Ready"),
        move |status| recorded.lock().unwrap().push(status),
    )
    .await;
    let failure = result.expect_err("corrupt SQLite must stop bootstrap");
    assert!(
        failure.chain().any(|cause| cause
            .downcast_ref::<shellfox_core::Error>()
            .is_some_and(|error| error.code == shellfox_core::ErrorCode::StorageFailed)),
        "{failure:#}"
    );
    {
        let recorded = statuses.lock().unwrap();
        assert!(recorded
            .iter()
            .any(|status| matches!(status, LocalBootStatus::RetainedCleanup)));
        assert_eq!(recorded.last(), Some(&LocalBootStatus::NoOwnerClean));
    }
    assert_eq!(std::fs::read(endpoint).ok(), stopped_endpoint);
    // Reacquisition supplements, never replaces, the explicit actor/helper
    // shutdown proof emitted by the bootstrap. This starts no tsnet/tailnet.
    let verified = VerifiedHelper::new(helper, digest).unwrap();
    let lock_directory =
        shellfox_transport::normalize_private_path(&std::fs::canonicalize(&data).unwrap()).unwrap();
    let reacquired = HostOwnership::acquire(&verified, &lock_directory)
        .await
        .unwrap()
        .expect("original ownership lock released");
    reacquired.shutdown().await.unwrap();
}
