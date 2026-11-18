#![cfg(feature = "test-helper")]
use sha2::{Digest, Sha256};
use shellfox_transport::*;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
#[path = "support/lifecycle.rs"]
mod lifecycle;
mod support;
use lifecycle::{lease_held, wait_file, wait_released, Release};
use support::{executable, Scratch};
fn config(scratch: &Scratch, mode: &str) -> (HelperConfig, PathBuf) {
    let state = IsolatedStateDirectory::create_new(scratch.0.join("node")).unwrap();
    let path = state.path().to_owned();
    (
        HelperConfig {
            executable: executable(),
            state_directory: state,
            hostname: mode.into(),
            backend: LoopbackOrigin::new("127.0.0.1:52744".parse().unwrap()).unwrap(),
            backend_secret: Secret::new("b".repeat(32)).unwrap(),
            proxy_secret: Secret::new("p".repeat(32)).unwrap(),
        },
        path,
    )
}
struct BundleLease(Arc<AtomicUsize>);
impl Drop for BundleLease {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
fn attempts(path: &Path) -> Vec<u8> {
    std::fs::read(path.join("stop-attempts")).unwrap()
}
#[tokio::test]
async fn lost_stop_reply_with_original_child_exit_is_confirmed_and_idempotent() {
    let scratch = Scratch::new();
    let (config, path) = config(&scratch, "stop-lost-exit");
    let _release = Release(path.clone());
    let helper = HelperTransport::start(config).await.unwrap();
    assert!(!helper.child_exit_confirmed().await.unwrap());
    let result = helper.shutdown_confirmed().await.unwrap();
    assert!(!result.stop_acknowledged);
    assert!(helper.child_exit_confirmed().await.unwrap());
    assert_eq!(helper.shutdown_confirmed().await.unwrap(), result);
    assert_eq!(attempts(&path), b"1\n");
    assert!(!lease_held(&path));
    drop(helper);
}
#[tokio::test]
async fn stop_ack_and_stdout_eof_cannot_claim_exit_of_a_live_original_child() {
    for (mode, ack) in [("stop-ack-live", true), ("stop-eof-live", false)] {
        let scratch = Scratch::new();
        let (mut config, path) = config(&scratch, mode);
        let dropped = Arc::new(AtomicUsize::new(0));
        config.executable = config
            .executable
            .with_resource_guard(Arc::new(BundleLease(dropped.clone())));
        let release = Release(path.clone());
        let helper = HelperTransport::start(config).await.unwrap();
        let pid = std::fs::read(path.join("original-pid")).unwrap();
        assert_eq!(
            helper.shutdown_confirmed().await,
            Err(HelperError::Deadline)
        );
        assert!(!helper.child_exit_confirmed().await.unwrap());
        assert!(lease_held(&path));
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
        release.now();
        let result = helper.shutdown_confirmed().await.unwrap();
        assert_eq!(result.stop_acknowledged, ack);
        assert!(helper.child_exit_confirmed().await.unwrap());
        assert_eq!(helper.shutdown_confirmed().await.unwrap(), result);
        assert_eq!(std::fs::read(path.join("original-pid")).unwrap(), pid);
        assert_eq!(attempts(&path), b"1\n");
        drop(helper);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while dropped.load(Ordering::SeqCst) == 0 {
            assert!(std::time::Instant::now() < deadline);
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn canceled_start_rolls_back_original_child_before_releasing_its_state_capability() {
    let scratch = Scratch::new();
    let (config, path) = config(&scratch, "start-hold");
    let _release = Release(path.clone());
    let start = tokio::spawn(HelperTransport::start(config));
    wait_file(&path.join("original-pid")).await;
    assert!(lease_held(&path));
    start.abort();
    assert!(start.await.unwrap_err().is_cancelled());
    wait_released(&path).await;
    // The state marker pin belongs to the retained startup bundle, not just
    // the future that was canceled. Wait until that confirmed owner releases it.
    let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let mut open = std::fs::OpenOptions::new();
        open.write(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            open.share_mode(0);
        }
        if open
            .open(path.join(IsolatedStateDirectory::MARKER_FILE))
            .is_ok()
        {
            break;
        }
        assert!(std::time::Instant::now() < end);
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}
#[tokio::test]
async fn returned_start_validation_error_retains_original_child_and_artifact_bundle() {
    let scratch = Scratch::new();
    let (mut config, path) = config(&scratch, "start-invalid");
    let _release = Release(path.clone());
    let dropped = Arc::new(AtomicUsize::new(0));
    config.executable = config
        .executable
        .with_resource_guard(Arc::new(BundleLease(dropped.clone())));
    assert_eq!(
        HelperTransport::start(config).await.unwrap_err(),
        HelperError::Protocol
    );
    wait_released(&path).await;
    let end = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while dropped.load(Ordering::SeqCst) == 0 {
        assert!(std::time::Instant::now() < end);
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn canceled_probe_keeps_an_original_child_controller_until_os_confirmation() {
    let scratch = Scratch::new();
    let _release = Release(scratch.0.clone());
    let source = PathBuf::from(env!("CARGO_BIN_EXE_shellfox-fixture-helper"));
    let copy = scratch.0.join(if cfg!(windows) {
        "probe-hold.exe"
    } else {
        "probe-hold"
    });
    std::fs::copy(source, &copy).unwrap();
    let digest: [u8; 32] = Sha256::digest(std::fs::read(&copy).unwrap()).into();
    let executable = VerifiedHelper::new(copy, digest).unwrap();
    let probe = tokio::spawn(async move { HelperTransport::probe(&executable).await });
    wait_file(&scratch.0.join("original-pid")).await;
    assert!(lease_held(&scratch.0));
    probe.abort();
    assert!(matches!(probe.await,Err(error) if error.is_cancelled()));
    wait_released(&scratch.0).await;
}
