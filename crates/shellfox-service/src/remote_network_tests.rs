//! Actual child-process fixture, not a Tailscale node or account enrollment.
use super::*;
use shellfox_transport::{LoopbackOrigin, Secret, VerifiedHelper};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
struct ResourceLease(Arc<AtomicBool>);
impl Drop for ResourceLease {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
fn executable() -> VerifiedHelper {
    let path = PathBuf::from(
        std::env::var_os("SHELLFOX_PROCESS_FIXTURE")
            .expect("explicit offline process fixture required"),
    );
    assert_eq!(
        path.file_stem().unwrap(),
        "shellfox-fixture-helper",
        "never point this test at the real network helper"
    );
    let hex =
        std::env::var("SHELLFOX_PROCESS_FIXTURE_SHA256").expect("explicit fixture digest required");
    assert_eq!(hex.len(), 64);
    let mut hash = [0; 32];
    for (i, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
    }
    VerifiedHelper::new(path, hash).unwrap()
}
#[tokio::test]
#[ignore = "requires explicit verified no-tsnet process fixture; never real helper start"]
async fn go_adapter_uses_original_child_exit_proof_and_accepts_already_exited_retry() {
    for mode in ["normal", "wrong-id"] {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        std::fs::create_dir_all(&root).unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        let scratch = tempfile::Builder::new()
            .prefix("remote-process-")
            .tempdir_in(root)
            .unwrap();
        let released = Arc::new(AtomicBool::new(false));
        let guard: Arc<dyn Send + Sync> = Arc::new(ResourceLease(released.clone()));
        let weak = Arc::downgrade(&guard);
        let config = HelperConfig {
            executable: executable().with_resource_guard(guard),
            state_directory: IsolatedStateDirectory::create_new(scratch.path().join("node"))
                .unwrap(),
            hostname: mode.into(),
            backend: LoopbackOrigin::new("127.0.0.1:9".parse().unwrap()).unwrap(),
            backend_secret: Secret::new("b".repeat(32)).unwrap(),
            proxy_secret: Secret::new("p".repeat(32)).unwrap(),
        };
        let helper = HelperTransport::start(config).await.unwrap();
        assert!(!helper.child_exit_confirmed().await.unwrap());
        assert!(weak.upgrade().is_some());
        assert!(!released.load(Ordering::SeqCst));
        if mode == "wrong-id" {
            assert!(helper.status().await.is_err());
            assert!(helper.child_exit_confirmed().await.unwrap());
        }
        let network = GoNetwork(helper);
        network.shutdown().await.unwrap();
        assert!(network.0.child_exit_confirmed().await.unwrap());
        network.shutdown().await.unwrap();
        assert!(network.0.child_exit_confirmed().await.unwrap());
        assert!(!released.load(Ordering::SeqCst));
        drop(network);
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while !released.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(weak.upgrade().is_none());
    }
}
