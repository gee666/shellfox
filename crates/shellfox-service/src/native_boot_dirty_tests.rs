use super::*;
use crate::{ActivityShutdownState, OwnerDispatcher, Service};
use shellfox_core::{CreateSession, ProfileId, SessionId};
use shellfox_transport::{HostOwnership, OwnershipDecision, VerifiedHelper};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[tokio::test(flavor = "current_thread")]
#[ignore = "explicit verified offline helper only; fresh private fixtures, no user host"]
async fn guard_clean_resources_preserve_actor_dirty_warning_after_go_cleanup() {
    struct Dirty {
        cwd: String,
        done: Arc<AtomicBool>,
        fired: bool,
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
                    title: None,
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
            self.done.store(true, Ordering::Release);
        }
    }
    let root =
        std::fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp")).unwrap();
    let data = tempfile::Builder::new()
        .prefix("native-dirty-guard-")
        .tempdir_in(root)
        .unwrap();
    let path =
        PathBuf::from(std::env::var_os("SHELLFOX_SERVICE_TEST_HELPER").expect("helper artifact"));
    let hash = std::env::var("SHELLFOX_SERVICE_TEST_HELPER_SHA256").expect("trusted digest");
    assert_eq!(hash.len(), 64);
    let mut digest = [0; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hash[index * 2..index * 2 + 2], 16).unwrap();
    }
    let private = PrivateDirectory::plan(data.path(), &path, digest, true).unwrap();
    let directory = shellfox_transport::normalize_private_path(data.path()).unwrap();
    let mut guard = BootGuard::new(private).unwrap();
    assert!(matches!(
        guard
            .preparation
            .acquire(&mut guard.private, &guard.server.stop)
            .await
            .unwrap(),
        OwnershipDecision::Acquired
    ));
    let done = Arc::new(AtomicBool::new(false));
    guard
        .start(Dirty {
            cwd: directory.to_string_lossy().into_owned(),
            done: done.clone(),
            fired: false,
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !done.load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    let stages = std::sync::Mutex::new(Vec::new());
    let outcome = guard
        .cleanup(&|status| stages.lock().unwrap().push(status))
        .await;
    assert_eq!(
        outcome.activity,
        ActivityShutdownState::DirtyAfterOwnershipLoss {
            pending_sessions: 1
        }
    );
    assert_eq!(
        stages.lock().unwrap().last(),
        Some(&LocalBootStatus::NoOwnerClean)
    );
    assert!(guard.actor.is_none() && guard.remote.is_none() && guard.listener.is_none());
    guard
        .preparation
        .cleanup()
        .await
        .expect("same original scope confirms cleanup");
    #[cfg(windows)]
    assert!(!data.path().join("rust-service-v1/endpoint.json").exists());
    #[cfg(unix)]
    assert_eq!(
        guard.endpoint_report().artifact_disposition(),
        crate::EndpointArtifactDisposition::Preserved
    );
    let verified = VerifiedHelper::new(path, digest).unwrap();
    let mut again = HostOwnership::acquire(&verified, &directory)
        .await
        .unwrap()
        .expect("confirmed resource release");
    again.shutdown_confirmed().await.unwrap();
}
