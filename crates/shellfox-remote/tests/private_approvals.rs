//! Legacy approval metadata is inspectable but cannot grant access or be rewritten.
use shellfox_test_support::native_metadata::Fixture;
use shellfox_protocol::ErrorCode;
use shellfox_remote::{Approval, ApprovalControl, PrivateApprovalStore, ViewPermission};
use std::{ffi::OsStr, sync::Arc};
fn current() -> Vec<u8> {
    br#"{"version":2,"approvals":[{"peerId":"n-owned","hostname":"fixture","approvedAtMillis":1,"policy":"control-full-account-v1"}]}"#.to_vec()
}
fn record() -> Approval {
    Approval {
        peer_id: "n-owned".into(),
        hostname: "fixture".into(),
        approved_at_millis: 1,
        permission: ViewPermission::Write,
        file_roots: Vec::new(),
    }
}
fn store(f: &Fixture) -> PrivateApprovalStore {
    let store = PrivateApprovalStore::retained_native(f.files.clone(), f.lost.clone());
    store.initialize_read_only().unwrap();
    store
}
fn child(f: &Fixture) -> Arc<shellfox_files::FileService> {
    f.files
        .private_child(OsStr::new("remote-v1"))
        .unwrap()
}
#[tokio::test]
async fn readonly_v2_keeps_exact_namespace_and_explicit_full_account_policy() {
    let f = Fixture::new();
    let directory = child(&f);
    Fixture::seed(&directory, "approvals.json", &current());
    let store = store(&f);
    let inventory = store.load().await.unwrap();
    assert_eq!(inventory.len(), 1);
    assert_eq!(inventory[0].permission, ViewPermission::Write);
    assert!(inventory[0].file_roots.is_empty());
    assert_eq!(
        ApprovalControl::load(&store).await.err().unwrap().code,
        ErrorCode::Unsupported,
        "native persisted inventory must not install a runtime grant"
    );
    assert!(!f.control_path().join("approvals.json").exists());
    assert!(f.control_path().join("remote-v1/approvals.json").exists());
}
#[tokio::test]
async fn every_legacy_pending_presence_denies_without_repair_or_deletion() {
    for marker in [
        &b""[..],
        &b"torn"[..],
        &br#"{"version":255,"phase":"committed"}"#[..],
    ] {
        let f = Fixture::new();
        let directory = child(&f);
        Fixture::seed(&directory, "approvals.json", &current());
        Fixture::seed(&directory, "write-pending", marker);
        let store = store(&f);
        assert!(store.load().await.is_err());
        assert!(ApprovalControl::load(&store).await.is_err());
        assert_eq!(
            directory
                .read_private(OsStr::new("write-pending"), 1024)
                .unwrap()
                .bytes,
            marker
        );
    }
}
#[tokio::test]
async fn gated_save_does_not_create_marker_stage_or_change_approvals() {
    let f = Fixture::new();
    let directory = child(&f);
    Fixture::seed(&directory, "approvals.json", &current());
    let store = store(&f);
    let before = std::fs::read_dir(f.control_path().join("remote-v1"))
        .unwrap()
        .count();
    let version = directory
        .read_private(OsStr::new("approvals.json"), 1024)
        .unwrap()
        .version;
    assert_eq!(
        store.save(&[record()]).await.unwrap_err().code,
        ErrorCode::StorageFailed
    );
    assert_eq!(
        directory
            .read_private(OsStr::new("approvals.json"), 1024)
            .unwrap()
            .version,
        version
    );
    assert_eq!(
        std::fs::read_dir(f.control_path().join("remote-v1"))
            .unwrap()
            .count(),
        before
    );
    assert!(!f.control_path().join("remote-v1/write-pending").exists());
}
#[tokio::test]
async fn v1_needing_save_cannot_install_a_normalized_grant_while_writes_are_gated() {
    let f = Fixture::new();
    let directory = child(&f);
    let legacy=br#"{"version":1,"approvals":[{"peerId":"n-owned","hostname":"fixture","approvedAtMillis":1,"permission":"read-only","fileRoots":[]}]}"#;
    Fixture::seed(&directory, "approvals.json", legacy);
    let store = store(&f);
    assert_eq!(
        ApprovalControl::load(&store).await.err().unwrap().code,
        ErrorCode::Unsupported
    );
    assert_eq!(
        directory
            .read_private(OsStr::new("approvals.json"), 1024)
            .unwrap()
            .bytes,
        legacy
    );
}
#[tokio::test]
async fn ownership_loss_denies_read_and_keeps_preparation_owner_for_disposition() {
    let f = Fixture::new();
    let directory = child(&f);
    Fixture::seed(&directory, "approvals.json", &current());
    let store = store(&f);
    f.lost.cancel();
    assert!(store.load().await.is_err());
}

#[tokio::test]
async fn failed_revoke_does_not_turn_legacy_inventory_into_runtime_authority() {
    let f = Fixture::new();
    let directory = child(&f);
    Fixture::seed(&directory, "approvals.json", &current());
    let first = store(&f);
    let control = ApprovalControl::default();
    assert_eq!(
        control.revoke(&first, "n-owned").await.unwrap_err().code,
        ErrorCode::StorageFailed
    );
    assert!(control.state().unwrap().approvals.is_empty());
    assert_eq!(
        control.set_enabled(true).unwrap_err().code,
        ErrorCode::Unsupported
    );
    let second = store(&f);
    assert_eq!(
        second.load().await.unwrap().len(),
        1,
        "readonly inventory remains, but is not runtime authority"
    );
    assert_eq!(
        ApprovalControl::load(&second).await.err().unwrap().code,
        ErrorCode::Unsupported
    );
    assert!(!f.control_path().join("remote-v1/write-pending").exists());
}
