use super::*;
use crate::native_metadata_fixture as native_metadata;
use crate::OwnerDispatcher;
use shellfox_remote::{owner_channel, ApprovalControl, PrivateApprovalStore, RemoteHost};
use shellfox_transport::{
    incoming::PeerIdentity, HostOwnership, PermissionVerifier, PrivatePathKind,
};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
struct Provider {
    started: AtomicBool,
    release: tokio::sync::Notify,
}
impl ManagerFactsProvider for Provider {
    fn collect(&self, request: ManagerFactsRequest) -> ManagerFactsFuture<'_> {
        Box::pin(async move {
            assert!(request.terminals.is_empty());
            self.started.store(true, Ordering::SeqCst);
            self.release.notified().await;
            Ok(ManagerFactsResult {
                facts: Default::default(),
                profiles: None,
                home: None,
            })
        })
    }
}
#[tokio::test]
#[ignore = "explicit verified offline helper required; no node or login"]
async fn async_facts_never_borrow_service_and_revision_changes_fence_mutations() {
    let parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&parent).unwrap();
    let parent = std::fs::canonicalize(parent).unwrap();
    #[cfg(windows)]
    let parent = std::path::PathBuf::from(
        parent
            .to_str()
            .unwrap()
            .strip_prefix(r"\\?\")
            .unwrap_or(parent.to_str().unwrap()),
    );
    let scratch = tempfile::Builder::new()
        .prefix("service-remote-")
        .tempdir_in(parent)
        .unwrap();
    let helper = super::super::tests::helper();
    PermissionVerifier::new(helper.clone())
        .protect(scratch.path(), PrivatePathKind::Directory)
        .await
        .unwrap();
    let ownership = HostOwnership::acquire(&helper, scratch.path())
        .await
        .unwrap()
        .unwrap();
    let metadata = native_metadata::Fixture::new();
    let directory = metadata
        .files
        .open_or_create_private_child(std::ffi::OsStr::new("remote-v1"))
        .unwrap();
    native_metadata::Fixture::seed(&directory,"approvals.json",br#"{"version":2,"approvals":[{"peerId":"n-facts","hostname":"offline","approvedAtMillis":1,"policy":"control-full-account-v1"}]}"#);
    assert!(metadata
        .control_path()
        .join("remote-v1/approvals.json")
        .exists());
    let store =
        PrivateApprovalStore::retained_native(metadata.files.clone(), metadata.lost.clone());
    store.initialize_read_only().unwrap();
    // Positive Service/Go metadata acceptance is PENDING native approval authority.
    let control = match ApprovalControl::load(&store).await {
        Ok(control) => control,
        Err(error) => {
            assert_eq!(error.code, wire::ErrorCode::Unsupported);
            ownership.shutdown().await.unwrap();
            return;
        }
    };
    let actor = control
        .authorize(
            &PeerIdentity {
                id: "n-facts".into(),
                hostname: "offline".into(),
            },
            wire::ClientId::new_v4(),
            1,
        )
        .unwrap();
    let mut service =
        Service::open_owned(scratch.path(), &std::env::current_exe().unwrap()).unwrap();
    let created = service
        .host()
        .create_session(shellfox_core::CreateSession {
            request_id: SessionId::new(),
            title: Some("Original".into()),
            cwd: scratch.path().to_str().unwrap().into(),
            profile_id: Default::default(),
        })
        .unwrap();
    let id = created.sessions[0].id;
    let (client, receiver) = owner_channel(1);
    let provider = Arc::new(Provider {
        started: AtomicBool::new(false),
        release: tokio::sync::Notify::new(),
    });
    let mut state = RemoteState::new(Default::default(), None, None);
    actor
        .with_authorized(None, || {
            shellfox_remote::ServiceHost::dispatch(
                &mut state.bind(&mut service),
                OwnerCommand::Pair {
                    actor: actor.clone(),
                },
            )
        })
        .unwrap();
    let mut dispatcher = super::super::RemoteDispatcher::new(receiver, state)
        .with_facts_provider(provider.clone(), tokio::runtime::Handle::current());
    let command = OwnerCommand::Execute {
        actor,
        command: wire::RemoteCommand::RenameSession(wire::RenameSession {
            session_id: id,
            title: "Remote mutation must not commit".into(),
        }),
    };
    let finished = AtomicBool::new(false);
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        tokio::join!(
            async {
                let result = client.request(command).await;
                finished.store(true, Ordering::SeqCst);
                result
            },
            async {
                while !provider.started.load(Ordering::SeqCst) {
                    dispatcher.dispatch_one(&mut service);
                    tokio::task::yield_now().await;
                }
                service.host().rename_session(id, "Local change").unwrap();
                provider.release.notify_one();
                while !finished.load(Ordering::SeqCst) {
                    dispatcher.dispatch_one(&mut service);
                    tokio::task::yield_now().await;
                }
            }
        )
    })
    .await
    .unwrap();
    assert_eq!(result.unwrap_err().code, ErrorCode::TargetLost);
    assert_eq!(
        service.host().snapshot().unwrap().sessions[0].title,
        "Local change"
    );
    service.shutdown().unwrap();
    drop(dispatcher);
    drop(service);
    ownership.shutdown().await.unwrap();
}
