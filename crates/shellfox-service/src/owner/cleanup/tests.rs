use super::*;
use crate::{owner::ServiceOwner, OwnerDispatcher};
use shellfox_core::{CreateSession, ProfileId, SessionId};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[test]
fn normal_persistence_error_retains_service_pending_touches_and_retries_only_metadata() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root).unwrap();
    let data = tempfile::Builder::new()
        .prefix("native-retained-activity-")
        .tempdir_in(root)
        .unwrap();
    let mut service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    let snapshot = service
        .host()
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: data.path().to_string_lossy().into_owned(),
            profile_id: ProfileId::new("cmd").unwrap(),
        })
        .unwrap();
    let id = snapshot.sessions[0].id;
    let at = shellfox_host::activity_unix_ms(&snapshot.sessions[0].updated_at).unwrap() + 1000;
    service.activity.persistence.touch(id, at).unwrap();
    let blocked = service.host().prepare_delete_session(id).unwrap();
    let mut retained = 0;
    let outcome = finish_with(service, |service, error| {
        assert!(matches!(error, ShutdownFailure::ActivityPersistence(_)));
        assert_eq!(service.active_terminals(), 0);
        assert_eq!(service.activity.persistence.pending_count(), 1);
        assert!(!service.ownership_lost.is_cancelled());
        retained += 1;
        service.host().cancel_close(blocked.token).unwrap();
    });
    assert_eq!(retained, 1);
    assert_eq!(outcome.activity, ActivityShutdownState::Committed);
    let reopened = shellfox_host::Host::open(data.path()).unwrap();
    assert_eq!(
        shellfox_host::activity_unix_ms(&reopened.snapshot().unwrap().sessions[0].updated_at)
            .unwrap(),
        at
    );
}

#[tokio::test(flavor = "current_thread")]
async fn actor_ack_and_join_preserve_dirty_after_loss_without_claiming_live_resources() {
    struct Dirty {
        cwd: String,
        fired: bool,
        completed: Arc<AtomicBool>,
    }
    impl OwnerDispatcher for Dirty {
        fn dispatch_one(&mut self, service: &mut Service) {
            if self.fired {
                return;
            }
            self.fired = true;
            let snapshot = service
                .host()
                .create_session(CreateSession {
                    request_id: SessionId::new(),
                    title: Some("owned activity fixture".into()),
                    cwd: self.cwd.clone(),
                    profile_id: ProfileId::new("cmd").unwrap(),
                })
                .unwrap();
            service
                .activity
                .persistence
                .touch(snapshot.sessions[0].id, 1)
                .unwrap();
            service.ownership_lost.cancel();
            self.completed.store(true, Ordering::Release);
        }
    }
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root).unwrap();
    let data = tempfile::Builder::new()
        .prefix("native-dirty-actor-")
        .tempdir_in(root)
        .unwrap();
    let completed = Arc::new(AtomicBool::new(false));
    let mut owner = ServiceOwner::start(
        data.path().to_owned(),
        std::env::current_exe().unwrap(),
        shellfox_protocol::CancellationToken::new(),
        Dirty {
            cwd: data.path().to_string_lossy().into_owned(),
            fired: false,
            completed: completed.clone(),
        },
    )
    .await
    .unwrap();
    owner.activate().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !completed.load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    let outcome = owner.shutdown().await.unwrap();
    assert_eq!(
        outcome.activity,
        ActivityShutdownState::DirtyAfterOwnershipLoss {
            pending_sessions: 1
        }
    );
    assert_eq!(owner.shutdown().await.unwrap(), outcome); // Cached positive ACK+join.
}

#[tokio::test(flavor = "current_thread")]
async fn startup_wait_cancellation_keeps_the_original_actor_controller() {
    use std::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    let data = tempfile::Builder::new()
        .prefix("native-startup-wait-")
        .tempdir_in(root)
        .unwrap();
    // Actual Service thread/SQLite owner, not a manufactured completion sender.
    let mut slot = Some(
        ServiceOwner::spawn_inner(
            data.path().to_owned(),
            std::env::current_exe().unwrap(),
            shellfox_protocol::CancellationToken::new(),
            None,
            (),
        )
        .unwrap(),
    );
    let owner = slot.as_mut().unwrap();
    assert!(owner.thread.is_some() && owner.started.is_some());
    let first = {
        let mut waiting = pin!(owner.wait_started());
        waiting
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
    };
    // A fast real startup may already be ready. Otherwise resume the same receiver.
    let startup = match first {
        Poll::Ready(result) => result,
        Poll::Pending => {
            assert!(owner.thread.is_some() && owner.started.is_some());
            owner.wait_started().await
        }
    };
    let shutdown = owner.shutdown().await;
    assert!(
        shutdown.is_ok(),
        "original actor ACK and join: {shutdown:?}"
    );
    assert!(
        startup.is_ok(),
        "startup result after original cleanup: {startup:?}"
    );
    assert!(owner.thread.is_none());
}
