use super::*;
use crate::host_client::HostEndpointStartupOutcome;
use std::path::PathBuf;
fn scratch(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join(format!(
        "{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&path).unwrap();
    path
}
async fn close(host: &AppHost) {
    host.cleanup_prestarted().await.unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn post_thread_failure_returns_exact_retained_host_not_occupied_or_outer_error() {
    let path = scratch("prestart-failure");
    let config = HostEndpointConfig {
        data_dir: path.clone(),
        helper: path.join("missing-helper.exe"),
        helper_sha256: [0; 32],
    };
    let AppHostPrestart::Failed { host, error } = AppHost::prestart(config).await.unwrap() else {
        panic!("typed failure required")
    };
    assert!(error.downcast_ref::<OccupiedOwnerError>().is_none());
    assert_eq!(
        host.startup_outcome().await.unwrap(),
        HostEndpointStartupOutcome::NoOwnerClean
    );
    close(&host).await;
    assert!(host.is_stopped().unwrap());
    std::fs::remove_dir_all(path).unwrap();
}
#[tokio::test(flavor = "current_thread")]
async fn invalid_relative_configuration_is_pre_thread_outer_error() {
    let config = HostEndpointConfig {
        data_dir: PathBuf::from("not-absolute"),
        helper: PathBuf::from("missing-helper"),
        helper_sha256: [0; 32],
    };
    assert!(AppHost::prestart(config).await.is_err());
}
#[tokio::test(flavor = "current_thread")]
#[ignore = "explicit verified offline helper; own directories, no GUI or account/network"]
async fn original_prestarted_host_is_ready_while_competing_attempt_has_sealed_occupied_proof() {
    let path = scratch("prestart-original");
    let helper =
        PathBuf::from(std::env::var_os("SHELLFOX_SERVICE_TEST_HELPER").expect("explicit helper"));
    let hex = std::env::var("SHELLFOX_SERVICE_TEST_HELPER_SHA256").expect("trusted hash");
    assert_eq!(hex.len(), 64);
    let mut digest = [0; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).unwrap();
    }
    let config = HostEndpointConfig {
        data_dir: path.clone(),
        helper,
        helper_sha256: digest,
    };
    let AppHostPrestart::Ready(host) = AppHost::prestart(config.clone()).await.unwrap() else {
        panic!("original owner")
    };
    let retained = host.clone();
    assert!(retained.same_owner(&host));
    let AppHostPrestart::Occupied(proof) = AppHost::prestart(config).await.unwrap() else {
        panic!("sealed occupied proof")
    };
    assert_eq!(
        std::fs::canonicalize(proof.directory()).unwrap(),
        std::fs::canonicalize(&path).unwrap()
    );
    assert_eq!(
        host.startup_outcome().await.unwrap(),
        HostEndpointStartupOutcome::Ready
    );
    assert_eq!(host.owned_terminal_count().await.unwrap(), 0);
    close(&host).await;
    assert!(retained.is_stopped().unwrap());
    std::fs::remove_dir_all(path).unwrap();
}
