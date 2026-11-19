//! The remote lifetime must be retained even when the metadata actor cannot boot.
use crate::{
    local::{serve_remote_with_boot_status, LocalBootStatus, RemoteBootOptions},
    remote::*,
};
use shellfox_transport::{
    HostOwnership, IsolatedStateDirectory, PermissionVerifier, PrivatePathKind, VerifiedHelper,
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
struct UnavailableSystem;
impl crate::system::SystemBackend for UnavailableSystem {
    fn home_directory(&self) -> crate::system::SystemResult<shellfox_protocol::Directory> {
        Err(shellfox_remote::capability_unavailable(
            "rollback fixture home",
        ))
    }
    fn cli_integration(
        &self,
        _: crate::system::SystemScope,
    ) -> crate::system::SystemResult<shellfox_protocol::CliIntegration> {
        Err(shellfox_remote::capability_unavailable(
            "rollback fixture launcher",
        ))
    }
    fn explorer_integration(
        &self,
        _: crate::system::SystemScope,
    ) -> crate::system::SystemResult<shellfox_protocol::ExplorerIntegration> {
        Err(shellfox_remote::capability_unavailable(
            "rollback fixture explorer",
        ))
    }
    fn perform_user_action(
        &self,
        _: crate::system::SystemAction,
    ) -> crate::system::SystemResult<crate::system::SystemResponse> {
        panic!("boot must never perform a system action")
    }
}
struct NoNetwork;
impl runtime::RemoteNetworkFactory for NoNetwork {
    fn start(
        &self,
        _: shellfox_transport::HelperConfig,
    ) -> runtime::NetworkFuture<'_, Arc<dyn runtime::RemoteNetwork>> {
        panic!("failed startup must never start a network node")
    }
}
#[tokio::test(flavor = "current_thread")]
async fn actor_start_failure_cleans_prepared_remote_before_releasing_native_ownership() {
    let root =
        std::fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp")).unwrap();
    let data = tempfile::Builder::new()
        .prefix("native-remote-rollback-")
        .tempdir_in(&root)
        .unwrap();
    let node = tempfile::Builder::new()
        .prefix("native-node-rollback-")
        .tempdir_in(&root)
        .unwrap();
    let (helper, digest) = crate::tests::helper();
    let verified = VerifiedHelper::new(helper.clone(), digest).unwrap();
    let verifier = PermissionVerifier::new(verified.clone());
    verifier
        .protect(node.path(), PrivatePathKind::Directory)
        .await
        .unwrap();
    let state_directory =
        IsolatedStateDirectory::create_new_private(node.path().join("node"), &verifier)
            .await
            .unwrap();
    std::fs::write(
        data.path().join(shellfox_host::STORE_FILENAME),
        b"deliberately corrupt sqlite fixture",
    )
    .unwrap();
    let system = Arc::new(crate::system::HostSystemServices::new(Arc::new(
        UnavailableSystem,
    )));
    let options = RemoteBootOptions::native(
        std::env::current_exe().unwrap(),
        system,
        crate::system::SystemScope::User,
        RemoteResources {
            state_directory,
            hostname: "offline-rollback-fixture".into(),
            network_factory: Arc::new(NoNetwork),
        },
    )
    .unwrap();
    let stages = Arc::new(Mutex::new(Vec::new()));
    let recorded = stages.clone();
    let result = serve_remote_with_boot_status(
        data.path(),
        &helper,
        digest,
        options,
        |_| panic!("corrupt actor cannot publish Ready"),
        move |stage| recorded.lock().unwrap().push(stage),
    )
    .await;
    assert!(result.is_err());
    {
        let stages = stages.lock().unwrap();
        assert!(!stages.iter().any(|stage| matches!(
            stage,
            LocalBootStatus::Ready | LocalBootStatus::ReadyLocalOnly { .. }
        )));
        assert_eq!(stages.last(), Some(&LocalBootStatus::NoOwnerClean));
    }
    let directory =
        shellfox_transport::normalize_private_path(&std::fs::canonicalize(data.path()).unwrap())
            .unwrap();
    let mut again = HostOwnership::acquire(&verified, &directory)
        .await
        .unwrap()
        .expect("native lock was released after remote shutdown");
    again.shutdown_confirmed().await.unwrap();
    assert!(!data.path().join("rust-service-v1/endpoint.json").exists());
}
