//! Bootstrap ownership proof, including errors before the public endpoint exists.
use crate::local::{serve_with_boot_status, LocalBootStatus};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

fn scratch() -> tempfile::TempDir {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root).unwrap();
    tempfile::Builder::new()
        .prefix("native-startup-proof-")
        .tempdir_in(root)
        .unwrap()
}
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
#[ignore = "requires explicit retained offline helper and SHA256; no production state or network"]
async fn failure_after_go_ownership_joins_actor_releases_helper_and_allows_lock_reacquire() {
    use shellfox_transport::{HostOwnership, VerifiedHelper};
    let data = scratch();
    // Pure filesystem/helper validation passes. Only Host::open on the owner
    // thread detects this failure, after the Go ownership lock was acquired.
    let corrupt = data.path().join(shellfox_host::STORE_FILENAME);
    std::fs::write(&corrupt, b"deliberately not a sqlite database").unwrap();
    let helper =
        PathBuf::from(std::env::var_os("SHELLFOX_SERVICE_TEST_HELPER").expect("explicit helper"));
    let hex = std::env::var("SHELLFOX_SERVICE_TEST_HELPER_SHA256").expect("explicit helper digest");
    assert_eq!(hex.len(), 64);
    let mut digest = [0; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).unwrap();
    }
    let statuses = Arc::new(Mutex::new(Vec::new()));
    let recorded = statuses.clone();
    let result = serve_with_boot_status(
        data.path(),
        &helper,
        digest,
        (),
        |_| panic!("failed boot cannot publish Ready"),
        move |status| recorded.lock().unwrap().push(status),
    )
    .await;
    assert!(result.is_err());
    {
        let recorded = statuses.lock().unwrap();
        assert!(recorded
            .iter()
            .any(|status| matches!(status, LocalBootStatus::RetainedCleanup)));
        assert_eq!(recorded.last(), Some(&LocalBootStatus::NoOwnerClean));
    }
    assert!(!data.path().join("rust-service-v1/endpoint.json").exists());
    // Reacquisition supplements, never replaces, the explicit actor/helper
    // shutdown proof emitted by the bootstrap. This starts no tsnet/tailnet.
    let verified = VerifiedHelper::new(helper, digest).unwrap();
    let lock_directory = shellfox_transport::normalize_private_path(&std::fs::canonicalize(data.path()).unwrap()).unwrap();
    let reacquired = HostOwnership::acquire(&verified, &lock_directory)
        .await
        .unwrap()
        .expect("original ownership lock released");
    reacquired.shutdown().await.unwrap();
}
