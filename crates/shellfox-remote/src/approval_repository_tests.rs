//! Repository-consumer ordering tests. Stub replies are NOT SQLite/native durability evidence.
use super::*;
use crate::{Approval, ApprovalControl, PrivateApprovalStore, ViewPermission};
use shellfox_core::{ApprovalStorePolicy, ApprovalStoreRecord, ApprovalStoreSnapshot};
use shellfox_protocol::{ClientId, ErrorCode};
use shellfox_transport::incoming::PeerIdentity;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use shellfox_test_support::native_metadata as metadata;
struct Repository {
    snapshot: Mutex<ApprovalStoreSnapshot>,
    fail: AtomicBool,
    loads: AtomicUsize,
    commits: AtomicUsize,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
impl Repository {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            snapshot: Mutex::new(ApprovalStoreSnapshot {
                revision: 0,
                approvals: vec![],
            }),
            fail: AtomicBool::new(false),
            loads: AtomicUsize::new(0),
            commits: AtomicUsize::new(0),
            entered: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
        })
    }
}
impl ApprovalRepository for Repository {
    fn load(&self) -> ApprovalRepositoryFuture<'_, ApprovalStoreSnapshot> {
        Box::pin(async move {
            self.loads.fetch_add(1, Ordering::SeqCst);
            Ok(self.snapshot.lock().unwrap().clone())
        })
    }
    fn commit(
        &self,
        expected: u64,
        mut records: Vec<ApprovalStoreRecord>,
    ) -> ApprovalRepositoryFuture<'_, ApprovalStoreSnapshot> {
        Box::pin(async move {
            self.commits.fetch_add(1, Ordering::SeqCst);
            self.entered.notify_one();
            self.release.notified().await;
            if self.fail.load(Ordering::SeqCst) {
                return Err(crate::error(
                    ErrorCode::StorageFailed,
                    "controlled repository failure",
                    false,
                ));
            }
            let mut current = self.snapshot.lock().unwrap();
            assert_eq!(current.revision, expected);
            records.sort_by(|a, b| a.peer_id.cmp(&b.peer_id));
            *current = ApprovalStoreSnapshot {
                revision: expected + 1,
                approvals: records,
            };
            Ok(current.clone())
        })
    }
}
fn approval() -> Approval {
    Approval {
        peer_id: "n-owned".into(),
        hostname: "owned".into(),
        approved_at_millis: 1,
        permission: ViewPermission::Write,
        file_roots: vec![],
    }
}
fn peer() -> PeerIdentity {
    PeerIdentity {
        id: "n-owned".into(),
        hostname: "owned".into(),
    }
}
fn store(fixture: &metadata::Fixture, repository: Arc<Repository>) -> Arc<PrivateApprovalStore> {
    let store = Arc::new(PrivateApprovalStore::retained_native(
        fixture.files.clone(),
        fixture.lost.clone(),
    ));
    store.bind_repository(repository).unwrap();
    store.initialize_read_only().unwrap();
    store
}
#[tokio::test]
async fn approve_installs_only_after_commit_reply_and_revoke_denies_before_io() {
    let fixture = metadata::Fixture::new();
    let repository = Repository::new();
    let store = store(&fixture, repository.clone());
    let control = ApprovalControl::load(&store).await.unwrap();
    let input = approval();
    let candidate = control.clone();
    let target = store.clone();
    let approving = tokio::spawn(async move { candidate.approve(&target, input).await });
    repository.entered.notified().await;
    assert!(control.authorize(&peer(), ClientId::new_v4(), 1).is_err());
    assert!(control.state().unwrap().approvals.is_empty());
    repository.release.notify_one();
    approving.await.unwrap().unwrap();
    let actor = control.authorize(&peer(), ClientId::new_v4(), 2).unwrap();
    repository.fail.store(true, Ordering::SeqCst);
    let candidate = control.clone();
    let target = store.clone();
    let revoking = tokio::spawn(async move { candidate.revoke(&target, "n-owned").await });
    repository.entered.notified().await;
    assert!(actor.with_authorized(None, || Ok(())).is_err());
    assert!(control.set_enabled(true).is_err());
    repository.release.notify_one();
    assert!(revoking.await.unwrap().is_err());
    assert!(actor.with_authorized(None, || Ok(())).is_err());
    assert!(control.authorize(&peer(), ClientId::new_v4(), 3).is_err());
    assert!(control.state().unwrap().approvals.is_empty());
    // A failed save keeps the policy closed: later changes never reach storage.
    let again = control.approve(&store, approval()).await.unwrap_err();
    assert_eq!(again.message, "Saving approvals failed. Restart Shellfox to retry.");
    assert_eq!(repository.commits.load(Ordering::SeqCst), 2);
    assert!(!fixture
        .control_path()
        .join("remote-v1/approvals.json")
        .exists());
}
#[tokio::test]
async fn pair_waits_for_approval_persistence_without_replaying_a_genuine_denial() {
    let fixture = metadata::Fixture::new();
    let repository = Repository::new();
    let store = store(&fixture, repository.clone());
    let control = ApprovalControl::load(&store).await.unwrap();
    let now = crate::backend::now_millis();
    let rejected = ClientId::new_v4();
    assert_eq!(
        control
            .authorize(&peer(), rejected, now)
            .err()
            .unwrap()
            .code,
        ErrorCode::ApprovalRequired
    );
    control.reject("n-owned").unwrap();
    let accepted = ClientId::new_v4();
    assert_eq!(
        control
            .authorize(&peer(), accepted, now)
            .err()
            .unwrap()
            .code,
        ErrorCode::ApprovalRequired
    );

    let candidate = control.clone();
    let approving = tokio::spawn(async move { candidate.approve(&store, approval()).await });
    repository.entered.notified().await;
    let mut waiting = Box::pin(control.authorize_pair_async(peer(), accepted, now));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(200), waiting.as_mut())
            .await
            .is_err()
    );
    repository.release.notify_one();
    approving.await.unwrap().unwrap();
    assert!(waiting.await.is_ok());
    assert_eq!(
        control
            .authorize_pair_async(peer(), rejected, now)
            .await
            .err()
            .unwrap()
            .code,
        ErrorCode::AccessDenied
    );
}

#[tokio::test]
async fn cancelled_management_keeps_policy_and_store_uncertain_without_retry() {
    let fixture = metadata::Fixture::new();
    let repository = Repository::new();
    let store = store(&fixture, repository.clone());
    let control = ApprovalControl::load(&store).await.unwrap();
    let candidate = control.clone();
    let target = store.clone();
    let changing = tokio::spawn(async move { candidate.approve(&target, approval()).await });
    repository.entered.notified().await;
    changing.abort();
    let _ = changing.await;
    assert!(control.set_enabled(true).is_err());
    assert!(control.authorize(&peer(), ClientId::new_v4(), 1).is_err());
    assert!(store.load().await.is_err());
    assert_eq!(repository.commits.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn legacy_pending_or_approval_presence_refuses_sql_authority_and_preserves_bytes() {
    for leaf in ["write-pending", "approvals.json"] {
        let fixture = metadata::Fixture::new();
        let repository = Repository::new();
        let store = store(&fixture, repository.clone());
        let directory = fixture
            .files
            .private_child(std::ffi::OsStr::new("remote-v1"))
            .unwrap();
        metadata::Fixture::seed(&directory, leaf, b"preserved legacy evidence");
        assert!(ApprovalControl::load(&store).await.is_err());
        assert_eq!(repository.loads.load(Ordering::SeqCst), 0);
        assert_eq!(
            directory
                .read_private(std::ffi::OsStr::new(leaf), 1024)
                .unwrap()
                .bytes,
            b"preserved legacy evidence"
        );
    }
}
#[test]
fn repository_dtos_are_closed_fixed_policy_and_bounded() {
    let record = ApprovalStoreRecord {
        peer_id: "n-owned".into(),
        hostname: "owned".into(),
        approved_at_millis: 1,
        policy: ApprovalStorePolicy::ControlFullAccountV1,
    };
    assert!(ApprovalStoreSnapshot {
        revision: 0,
        approvals: vec![record.clone(), record]
    }
    .validate()
    .is_err());
    assert!(serde_json::from_str::<ApprovalStoreRecord>(
        r#"{"peerId":"n-owned","hostname":"owned","approvedAtMillis":1,"policy":"future-admin"}"#
    )
    .is_err());
}
