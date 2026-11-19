#![cfg(feature = "test-helper")]
use shellfox_transport::*;
use std::time::{Duration, Instant};
#[path = "support/lifecycle.rs"]
mod lifecycle;
mod support;
use lifecycle::{lease_held, wait_file, wait_released, Release};
use support::{executable, Scratch};

#[tokio::test]
async fn timeout_and_protocol_error_keep_original_controller_for_confirmation_retry() {
    for (name, error) in [
        ("ownership-delay", HelperError::Deadline),
        ("ownership-error", HelperError::Command),
    ] {
        let scratch = Scratch::new();
        let path = scratch.0.join(name);
        std::fs::create_dir(&path).unwrap();
        let mut owner = HostOwnership::acquire(&executable(), &path)
            .await
            .unwrap()
            .unwrap();
        let release = Release(path.clone());
        wait_file(&path.join("original-pid")).await;
        let pid = std::fs::read(path.join("original-pid")).unwrap();
        assert!(lease_held(&path));
        assert_eq!(owner.shutdown_confirmed().await, Err(error));
        assert!(lease_held(&path)); // EOF / lost health is not original-child exit.
        wait_file(&path.join("stdin-closed-once")).await;
        assert_eq!(std::fs::read(path.join("stdin-closed-once")).unwrap(), b"1");
        release.now();
        // Opening the fixture gate does not prove that the child has exited yet.
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match owner.shutdown_confirmed().await {
                    Err(HelperError::Command) if name == "ownership-error" => {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                    result => break result,
                }
            }
        })
        .await
        .expect("released child did not exit")
        .unwrap();
        owner.shutdown_confirmed().await.unwrap(); // Cached, idempotent original proof.
        wait_released(&path).await;
        assert_eq!(std::fs::read(path.join("original-pid")).unwrap(), pid);
        assert_eq!(std::fs::read(path.join("acquisitions")).unwrap(), b"1\n");
        drop(owner);
    }
}
#[test]
fn consuming_error_keeps_original_cleanup_owner_after_tokio_runtime_is_gone() {
    let scratch = Scratch::new();
    let path = scratch.0.join("ownership-delay");
    std::fs::create_dir(&path).unwrap();
    let release = Release(path.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let owner = runtime
        .block_on(HostOwnership::acquire(&executable(), &path))
        .unwrap()
        .unwrap();
    assert_eq!(
        runtime.block_on(owner.shutdown()),
        Err(HelperError::Deadline)
    );
    // Windows redirected-pipe reads use Tokio's blocking pool. Do not wait
    // for those pending reads while deliberately keeping the fixture alive.
    runtime.shutdown_background();
    std::thread::sleep(Duration::from_millis(50));
    assert!(lease_held(&path)); // Runtime drop must not destroy the controller.
    release.now();
    let deadline = Instant::now() + Duration::from_secs(5);
    while lease_held(&path) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(std::fs::read(path.join("acquisitions")).unwrap(), b"1\n");
}
