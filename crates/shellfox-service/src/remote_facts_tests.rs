use super::super::tests::{approval_owner, approve_peer};
use super::*;
use crate::native_metadata_fixture as native_metadata;
use crate::OwnerDispatcher;
use shellfox_remote::RemoteHost;
use shellfox_transport::incoming::PeerIdentity;
use std::sync::atomic::{AtomicBool, Ordering};
struct Provider {
    started: AtomicBool,
    release: tokio::sync::Notify,
}
impl super::super::SourceFactsProjector for Provider {
    fn project(
        &self,
        source: Arc<crate::activity_integration::SourceFacts>,
        request: ManagerFactsRequest,
    ) -> ManagerFactsFuture<'_> {
        Box::pin(async move {
            assert!(source.matches_request(&request));
            assert_eq!(source.snapshot.sessions[0].title, "Original");
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
async fn facts_refresh_before_projection_and_fence_mutations_after_it_starts() {
    let scratch = crate::tests::scratch();
    let approval_data = crate::tests::scratch();
    let mut owner = approval_owner(approval_data.path()).await;
    let metadata = native_metadata::Fixture::new();
    let peer = PeerIdentity {
        id: "n-facts".into(),
        hostname: "offline".into(),
    };
    let (control, _store) = approve_peer(&owner, &metadata, &peer).await;
    let actor = control
        .authorize(&peer, wire::ClientId::new_v4(), 1)
        .unwrap();
    let mut service =
        Service::open_owned(scratch.path(), &std::env::current_exe().unwrap()).unwrap();
    service.pump_activity();
    let watch = service.source_facts_watch();
    assert!(crate::activity_integration::read_source(&watch)
        .unwrap()
        .snapshot
        .sessions
        .is_empty());
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
    let (client, receiver) = super::super::remote_channel();
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
    let slot = Arc::new(super::super::SourceFactsProviderSlot::new(provider.clone()));
    slot.install(watch).unwrap();
    let mut dispatcher = super::super::RemoteDispatcher::new(receiver, state)
        .with_facts_provider(slot, tokio::runtime::Handle::current());
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
    owner.shutdown().await.unwrap();
}
