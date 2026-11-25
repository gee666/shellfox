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
        source: Arc<crate::activity::integration::SourceFacts>,
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
    assert!(crate::activity::integration::read_source(&watch)
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

/// Counts projections; returns empty facts at once.
#[derive(Default)]
struct Counting(std::sync::atomic::AtomicUsize);
impl super::super::SourceFactsProjector for Counting {
    fn project(
        &self,
        _: Arc<crate::activity::integration::SourceFacts>,
        _: ManagerFactsRequest,
    ) -> ManagerFactsFuture<'_> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Ok(ManagerFactsResult {
                facts: Default::default(),
                profiles: None,
                home: None,
            })
        })
    }
}

#[tokio::test]
async fn a_facts_request_during_a_close_is_held_then_collects_fresh_facts() {
    let scratch = crate::tests::scratch();
    let approval_data = crate::tests::scratch();
    let mut owner = approval_owner(approval_data.path()).await;
    let metadata = native_metadata::Fixture::new();
    let peer = PeerIdentity {
        id: "n-held-facts".into(),
        hostname: "offline".into(),
    };
    let (control, _store) = approve_peer(&owner, &metadata, &peer).await;
    let actor = control
        .authorize(&peer, wire::ClientId::new_v4(), 1)
        .unwrap();
    let mut service =
        Service::open_owned(scratch.path(), &std::env::current_exe().unwrap()).unwrap();
    let create = |title: &str| shellfox_core::CreateSession {
        request_id: SessionId::new(),
        title: Some(title.into()),
        cwd: scratch.path().to_str().unwrap().into(),
        profile_id: Default::default(),
    };
    let renamed = service
        .host()
        .create_session(create("Original"))
        .unwrap()
        .sessions[0]
        .id;
    let closing = service.host().create_session(create("Closing")).unwrap();
    let closing_tab = closing
        .sessions
        .iter()
        .find(|s| s.id != renamed)
        .unwrap()
        .tabs[0]
        .id;
    service.pump_activity();
    let (client, receiver) = super::super::remote_channel();
    let provider = Arc::new(Counting::default());
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
    slot.install(service.source_facts_watch()).unwrap();
    let mut dispatcher = super::super::RemoteDispatcher::new(receiver, state)
        .with_facts_provider(slot, tokio::runtime::Handle::current());
    let _outcome = service.park_close(closing_tab);
    let command = OwnerCommand::Execute {
        actor,
        command: wire::RemoteCommand::RenameSession(wire::RenameSession {
            session_id: renamed,
            title: "Renamed by the peer".into(),
        }),
    };
    let finished = AtomicBool::new(false);
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(
            async {
                let result = client.request(command).await;
                finished.store(true, Ordering::SeqCst);
                result
            },
            async {
                for _ in 0..20 {
                    dispatcher.dispatch_one(&mut service);
                    tokio::task::yield_now().await;
                }
                assert_eq!(
                    provider.0.load(Ordering::SeqCst),
                    0,
                    "no facts collected mid-close"
                );
                assert!(
                    !finished.load(Ordering::SeqCst),
                    "the request waits for the close"
                );
                // The commit moves the revision that a mid-close collection would be fenced on.
                service.unpark_close();
                service.pump_closing();
                assert!(!service.metadata_busy());
                while !finished.load(Ordering::SeqCst) {
                    dispatcher.dispatch_one(&mut service);
                    tokio::task::yield_now().await;
                }
            }
        )
    })
    .await
    .unwrap();
    let code = result.as_ref().err().map(|error| error.code);
    assert_ne!(code, Some(ErrorCode::TargetLost), "{result:?}");
    assert!(
        provider.0.load(Ordering::SeqCst) >= 1,
        "facts were collected after the commit"
    );
    let snapshot = service.host().snapshot().unwrap();
    assert!(snapshot
        .sessions
        .iter()
        .all(|s| s.tabs.iter().all(|t| t.id != closing_tab)));
    service.shutdown().unwrap();
    drop(dispatcher);
    drop(service);
    owner.shutdown().await.unwrap();
}

/// Holds its first capability projection until released; later ones return at once.
#[derive(Default)]
struct StallFirst {
    calls: std::sync::atomic::AtomicUsize,
    release: tokio::sync::Notify,
}
impl super::super::SourceFactsProjector for StallFirst {
    fn project(
        &self,
        _: Arc<crate::activity::integration::SourceFacts>,
        _: ManagerFactsRequest,
    ) -> ManagerFactsFuture<'_> {
        Box::pin(async { Err(capability_unavailable("test")) })
    }
    fn project_capabilities(
        &self,
        _: Arc<crate::activity::integration::SourceFacts>,
        _: ManagerFactsRequest,
    ) -> ManagerFactsFuture<'_> {
        let first = self.calls.fetch_add(1, Ordering::SeqCst) == 0;
        Box::pin(async move {
            if first {
                self.release.notified().await;
            }
            Ok(ManagerFactsResult {
                facts: Default::default(),
                profiles: None,
                home: None,
            })
        })
    }
}

#[tokio::test]
async fn a_file_request_collects_again_when_the_snapshot_changes_underneath() {
    let scratch = crate::tests::scratch();
    let approval_data = crate::tests::scratch();
    let mut owner = approval_owner(approval_data.path()).await;
    let metadata = native_metadata::Fixture::new();
    let peer = PeerIdentity {
        id: "n-file-facts".into(),
        hostname: "offline".into(),
    };
    let (control, _store) = approve_peer(&owner, &metadata, &peer).await;
    let actor = control
        .authorize(&peer, wire::ClientId::new_v4(), 1)
        .unwrap();
    let mut service =
        Service::open_owned(scratch.path(), &std::env::current_exe().unwrap()).unwrap();
    let id = service
        .host()
        .create_session(shellfox_core::CreateSession {
            request_id: SessionId::new(),
            title: Some("Original".into()),
            cwd: scratch.path().to_str().unwrap().into(),
            profile_id: Default::default(),
        })
        .unwrap()
        .sessions[0]
        .id;
    service.pump_activity();
    let (client, receiver) = super::super::remote_channel();
    let provider = Arc::new(StallFirst::default());
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
    slot.install(service.source_facts_watch()).unwrap();
    let mut dispatcher = super::super::RemoteDispatcher::new(receiver, state)
        .with_facts_provider(slot, tokio::runtime::Handle::current());
    let command = OwnerCommand::Execute {
        actor,
        command: wire::RemoteCommand::ListFiles(wire::ListFiles {
            session_id: Some(id),
            tab_id: None,
            path: ".".into(),
        }),
    };
    let finished = AtomicBool::new(false);
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(
            async {
                let result = client.request(command).await;
                finished.store(true, Ordering::SeqCst);
                result
            },
            async {
                while provider.calls.load(Ordering::SeqCst) == 0 {
                    dispatcher.dispatch_one(&mut service);
                    tokio::task::yield_now().await;
                }
                // Stands in for the minute activity touch: the snapshot moves on.
                service.host().rename_session(id, "Touched").unwrap();
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
    let code = result.as_ref().err().map(|error| error.code);
    assert_ne!(code, Some(ErrorCode::TargetLost), "{result:?}");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    service.shutdown().unwrap();
    drop(dispatcher);
    drop(service);
    owner.shutdown().await.unwrap();
}
