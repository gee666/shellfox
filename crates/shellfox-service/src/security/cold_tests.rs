use super::*;
use crate::local::{serve_with_boot_status, LocalBootStatus};
use shellfox_transport::{PermissionVerifier, PrivatePathKind};
use std::sync::{Arc, Mutex};
fn scratch() -> tempfile::TempDir {
    let root =
        std::fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp")).unwrap();
    tempfile::Builder::new()
        .prefix("native-cold-root-")
        .tempdir_in(root)
        .unwrap()
}
use crate::tests::helper;
#[tokio::test(flavor = "current_thread")]
async fn missing_client_root_never_creates_or_reaches_native_helpers() {
    let data = scratch();
    let path = data.path().join("never-create").join("host");
    let handled = std::sync::atomic::AtomicBool::new(false);
    let result = PrivateDirectory::prepare_reported(
        &path,
        &std::env::current_exe().unwrap(),
        [0; 32],
        false,
        || {
            handled.store(true, std::sync::atomic::Ordering::SeqCst);
        },
    )
    .await;
    assert!(result.is_err());
    assert!(!handled.load(std::sync::atomic::Ordering::SeqCst));
    assert!(!data.path().join("never-create").exists());
}
#[tokio::test(flavor = "current_thread")]
async fn foreign_or_production_root_is_rejected_before_permission_children_or_layout() {
    let data = scratch();
    let marker = data.path().join("manager.sqlite3");
    std::fs::write(&marker, b"do-not-touch").unwrap();
    let handled = std::sync::atomic::AtomicBool::new(false);
    let result = PrivateDirectory::prepare_reported(
        data.path(),
        &std::env::current_exe().unwrap(),
        [0; 32],
        true,
        || {
            handled.store(true, std::sync::atomic::Ordering::SeqCst);
        },
    )
    .await;
    assert!(result
        .err()
        .unwrap()
        .to_string()
        .contains("no permissions were changed"));
    assert!(!handled.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(std::fs::read(marker).unwrap(), b"do-not-touch");
    assert!(
        !data.path().join(ENDPOINT_DIRECTORY).exists()
            && !data.path().join("terminal-home").exists()
    );
}
#[tokio::test(flavor = "current_thread")]
async fn native_worker_creates_new_root_ready_and_removes_its_endpoint_at_cleanup() {
    cold_root_cycle(false).await;
}
#[tokio::test(flavor = "current_thread")]
async fn an_endpoint_another_run_replaced_is_kept_at_cleanup() {
    cold_root_cycle(true).await;
}
/// `replaced`: another run publishes over this run's endpoint.json before Stop.
async fn cold_root_cycle(replaced: bool) {
    let data = scratch();
    let (helper, digest) = helper();
    let verifier = PermissionVerifier::new(VerifiedHelper::new(helper.clone(), digest).unwrap());
    verifier
        .protect(data.path(), PrivatePathKind::Directory)
        .await
        .unwrap();
    let root = data.path().join("fresh").join("host");
    assert!(!root.exists());
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let (ready, started) = tokio::sync::oneshot::channel();
            let stages = Arc::new(Mutex::new(Vec::new()));
            let recorded = stages.clone();
            let target = root.clone();
            let artifact = helper.clone();
            let server = tokio::task::spawn_local(async move {
                serve_with_boot_status(
                    &target,
                    &artifact,
                    digest,
                    (),
                    move |_| {
                        let _ = ready.send(());
                    },
                    move |status| recorded.lock().unwrap().push(status),
                )
                .await
            });
            tokio::time::timeout(std::time::Duration::from_secs(30), started)
                .await
                .unwrap()
                .unwrap();
            assert!(
                root.join("rust-service-v1/endpoint.json").is_file()
                    && root.join("terminal-home").is_dir()
            );
            let client = crate::local::LocalClient::connect(&root, &helper, digest)
                .await
                .unwrap();
            let endpoint = root.join("rust-service-v1/endpoint.json");
            let other = br#"{"other":"run"}"#;
            if replaced {
                let dir = cap_std::fs::Dir::open_ambient_dir(
                    root.join("rust-service-v1"),
                    cap_std::ambient_authority(),
                )
                .unwrap()
                .into_std_file();
                let name = std::ffi::OsStr::new("endpoint.json");
                let ours = shellfox_files::private::read(&dir, name, 4096).unwrap();
                shellfox_files::private::write(&dir, name, other, Some(&ours.version)).unwrap();
            }
            client
                .execute(crate::experiment::ExperimentRequest::Stop {})
                .await
                .unwrap();
            server.await.unwrap().unwrap();
            assert_eq!(
                stages.lock().unwrap().last(),
                Some(&LocalBootStatus::NoOwnerClean)
            );
            if replaced {
                assert_eq!(std::fs::read(&endpoint).unwrap(), other, "another run's record is kept");
            } else {
                assert!(!endpoint.exists(), "this run's record is removed");
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn occupied_prepare_does_not_create_private_layout_or_ui_children() {
    let data = scratch();
    let (helper, digest) = helper();
    let private = PrivateDirectory::prepare(data.path(), &helper, digest, true)
        .await
        .unwrap();
    let root = shellfox_transport::normalize_private_path(data.path()).unwrap();
    let mut owner = shellfox_transport::HostOwnership::acquire(&private.helper, &root)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !data.path().join(ENDPOINT_DIRECTORY).exists()
            && !data.path().join("terminal-home").exists()
    );
    let result = serve_with_boot_status(
        data.path(),
        &helper,
        digest,
        (),
        |_| panic!("occupied Ready"),
        |_| {},
    )
    .await;
    assert!(result
        .unwrap_err()
        .downcast_ref::<crate::local::OccupiedOwnerError>()
        .is_some());
    assert!(
        !data.path().join(ENDPOINT_DIRECTORY).exists()
            && !data.path().join("terminal-home").exists()
    );
    assert!(owner.is_alive());
    owner.shutdown_confirmed().await.unwrap();
}
