//! Real SQLite/actor bridge tests, not Native-root/Go/authentication acceptance.
use super::*;
use crate::native_metadata_fixture::Fixture;
use shellfox_remote::{Approval, ApprovalControl, PrivateApprovalStore, ViewPermission};
use shellfox_transport::incoming::PeerIdentity;
use std::{
    future::Future,
    pin::pin,
    task::{Context as TaskContext, Poll, Waker},
};
fn scratch() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("owner-approval-")
        .tempdir_in(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp"))
        .unwrap()
}
fn record() -> ApprovalStoreRecord {
    ApprovalStoreRecord {
        peer_id: "n-owner".into(),
        hostname: "owned".into(),
        approved_at_millis: 1,
        policy: shellfox_core::ApprovalStorePolicy::ControlFullAccountV1,
    }
}
fn approval() -> Approval {
    Approval {
        peer_id: "n-owner".into(),
        hostname: "owned".into(),
        approved_at_millis: 1,
        permission: ViewPermission::Write,
        file_roots: vec![],
    }
}
fn app<T>(value: std::result::Result<T, AppError>) -> Result<T> {
    value.map_err(|error| anyhow::anyhow!(error.message))
}
async fn owner(path: &std::path::Path, lost: CancellationToken) -> ServiceOwner {
    ServiceOwner::start(path.to_owned(), std::env::current_exe().unwrap(), lost, ())
        .await
        .unwrap()
}
#[tokio::test(flavor = "current_thread")]
async fn original_inactive_actor_repository_drives_actual_store_approve_revoke_restart() {
    let fixture = Fixture::new();
    let data = scratch();
    let mut owner = owner(data.path(), fixture.lost.clone()).await;
    let repository = owner
        .client(uuid::Uuid::new_v4())
        .approval_repository(fixture.lost.clone());
    let store = PrivateApprovalStore::retained_native(fixture.files.clone(), fixture.lost.clone());
    let outcome = async {
        app(store.bind_repository(repository.clone()))?;
        app(store.initialize_read_only())?;
        let policy =
            app(
                tokio::time::timeout(Duration::from_secs(5), ApprovalControl::load(&store)).await?,
            )?;
        app(policy.approve(&store, approval()).await)?;
        let peer = PeerIdentity {
            id: "n-owner".into(),
            hostname: "owned".into(),
        };
        let actor = app(policy.authorize(&peer, uuid::Uuid::new_v4(), 1))?;
        let committed = app(repository.load().await)?;
        app(policy.revoke(&store, &peer.id).await)?;
        let denied = actor.with_authorized(None, || Ok(())).is_err();
        Ok::<_, anyhow::Error>((committed, app(repository.load().await)?, denied))
    }
    .await;
    let stopped = owner.shutdown().await;
    assert!(stopped.is_ok(), "{stopped:?}");
    let (committed, final_snapshot, denied) = outcome.unwrap();
    assert_eq!(committed.approvals, vec![record()]);
    assert!(denied);
    let host = shellfox_host::Host::open(data.path()).unwrap();
    assert_eq!(host.approval_snapshot().unwrap(), final_snapshot);
    assert!(final_snapshot.approvals.is_empty());
}
struct PausedDispatcher {
    entered: Option<oneshot::Sender<()>>,
    resume: std::sync::mpsc::Receiver<()>,
}
impl OwnerDispatcher for PausedDispatcher {
    fn dispatch_one(&mut self, _: &mut Service) {
        if let Some(entered) = self.entered.take() {
            let _ = entered.send(());
            let _ = self.resume.recv();
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn cancelled_accepted_repository_wait_does_not_cancel_sqlite_commit_history() {
    let data = scratch();
    let lost = CancellationToken::new();
    let (entered, paused) = oneshot::channel();
    let (resume, released) = std::sync::mpsc::channel();
    let mut owner = ServiceOwner::start(
        data.path().to_owned(),
        std::env::current_exe().unwrap(),
        lost.clone(),
        PausedDispatcher {
            entered: Some(entered),
            resume: released,
        },
    )
    .await
    .unwrap();
    let start = owner.activate().await;
    if start.is_ok() {
        let _ = paused.await;
    }
    let repository = owner.client(uuid::Uuid::new_v4()).approval_repository(lost);
    let first = {
        let mut waiting = pin!(repository.commit(0, vec![record()]));
        waiting
            .as_mut()
            .poll(&mut TaskContext::from_waker(Waker::noop()))
    };
    let _ = resume.send(());
    let after = repository.load().await;
    let stopped = owner.shutdown().await;
    assert!(stopped.is_ok(), "{stopped:?}");
    assert!(start.is_ok());
    assert!(
        matches!(first, Poll::Pending),
        "actor held until wait was cancelled"
    );
    let after = after.unwrap();
    assert_eq!(after.revision, 1);
    assert_eq!(after.approvals, vec![record()]);
    let host = shellfox_host::Host::open(data.path()).unwrap();
    assert_eq!(host.approval_snapshot().unwrap(), after);
}
fn dispatcher() -> Box<crate::remote::RemoteDispatcher> {
    let (_, receiver) = shellfox_remote::owner_channel(1);
    Box::new(crate::remote::RemoteDispatcher::new(
        receiver,
        crate::remote::RemoteState::new(Default::default(), None, None),
    ))
}
#[tokio::test(flavor = "current_thread")]
async fn remote_install_wait_cancellation_retains_same_dispatcher_and_receipt() {
    let data = scratch();
    let mut owner = owner(data.path(), CancellationToken::new()).await;
    let install = RemoteInstall::new();
    let prepared = install.prepare(dispatcher());
    let closing = shellfox_protocol::CancellationToken::new();
    let first = {
        let mut waiting = pin!(install.install(&owner, &closing));
        waiting
            .as_mut()
            .poll(&mut TaskContext::from_waker(Waker::noop()))
    };
    let resumed = install.install(&owner, &closing).await;
    let stopped = owner.shutdown().await;
    assert!(stopped.is_ok(), "{stopped:?}");
    assert!(prepared.is_ok());
    if let Poll::Ready(result) = first {
        assert!(result.is_ok(), "{result:?}");
    }
    assert!(resumed.is_ok(), "{resumed:?}");
    assert!(install.state.lock().unwrap().dispatcher.is_some());
    assert!(install.state.lock().unwrap().queued);
}
#[tokio::test(flavor = "current_thread")]
async fn preactivate_only_install_refusal_preserves_original_dispatcher() {
    let data = scratch();
    let mut owner = owner(data.path(), CancellationToken::new()).await;
    let activated = owner.activate().await;
    let install = RemoteInstall::new();
    let prepared = install.prepare(dispatcher());
    let closing = shellfox_protocol::CancellationToken::new();
    let refused = install.install(&owner, &closing).await;
    let stopped = owner.shutdown().await;
    assert!(stopped.is_ok(), "{stopped:?}");
    assert!(activated.is_ok() && prepared.is_ok() && refused.is_err());
    assert!(install.state.lock().unwrap().dispatcher.is_some());
}
