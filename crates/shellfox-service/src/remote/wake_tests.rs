use super::tests::{approval_owner, approve_peer, dispatch};
use super::*;
use crate::native_metadata_fixture as native_metadata;
use shellfox_remote::RemoteHost;
use shellfox_transport::incoming::PeerIdentity;

#[tokio::test]
async fn host_revision_wakes_every_view_until_it_polls() {
    let scratch = crate::tests::scratch();
    let approval_data = crate::tests::scratch();
    let mut owner = approval_owner(approval_data.path()).await;
    let metadata = native_metadata::Fixture::new();
    let peer = PeerIdentity {
        id: "n-wake".into(),
        hostname: "offline".into(),
    };
    let (approvals, _store) = approve_peer(&owner, &metadata, &peer).await;
    let first = approvals
        .authorize(&peer, wire::ClientId::new_v4(), 1)
        .unwrap();
    let second = approvals
        .authorize(&peer, wire::ClientId::new_v4(), 1)
        .unwrap();
    let mut service =
        Service::open_owned(scratch.path(), &std::env::current_exe().unwrap()).unwrap();
    let mut state = RemoteState::new(Default::default(), None, None);
    for actor in [&first, &second] {
        dispatch(
            &mut service,
            &mut state,
            OwnerCommand::Pair {
                actor: actor.clone(),
            },
        )
        .await
        .unwrap();
    }
    assert_eq!(state.ready_views(&service).count(), 0);
    service
        .host()
        .create_session(shellfox_core::CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: scratch.path().to_str().unwrap().into(),
            profile_id: Default::default(),
        })
        .unwrap();
    assert_eq!(state.ready_views(&service).count(), 2);
    let polled = dispatch(
        &mut service,
        &mut state,
        OwnerCommand::Poll {
            actor: first.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(polled["events"].as_array().unwrap().len(), 1);
    let ready: Vec<_> = state
        .ready_views(&service)
        .map(|view| view.controller)
        .collect();
    assert_eq!(ready, [state.views[&key(&second)].controller]);
    service.shutdown().unwrap();
    drop(service);
    owner.shutdown().await.unwrap();
}

#[tokio::test]
async fn tab_activity_changes_reach_each_view_once_as_a_native_change() {
    let scratch = crate::tests::scratch();
    let approval_data = crate::tests::scratch();
    let mut owner = approval_owner(approval_data.path()).await;
    let metadata = native_metadata::Fixture::new();
    let peer = PeerIdentity {
        id: "n-activity".into(),
        hostname: "offline".into(),
    };
    let (approvals, _store) = approve_peer(&owner, &metadata, &peer).await;
    let actor = approvals
        .authorize(&peer, wire::ClientId::new_v4(), 1)
        .unwrap();
    let mut service =
        Service::open_owned(scratch.path(), &std::env::current_exe().unwrap()).unwrap();
    let mut state = RemoteState::new(Default::default(), None, None);
    dispatch(
        &mut service,
        &mut state,
        OwnerCommand::Pair {
            actor: actor.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        state.ready_views(&service).count(),
        0,
        "pairing starts at the current activity"
    );
    service.replace_tab_activity(Vec::new());
    assert_eq!(state.ready_views(&service).count(), 1);
    let polled = dispatch(
        &mut service,
        &mut state,
        OwnerCommand::Poll {
            actor: actor.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        polled["events"],
        serde_json::json!([{"type":"changed","event":{"revision":0,"reason":"native"}}])
    );
    assert_eq!(
        state.ready_views(&service).count(),
        0,
        "one event per change"
    );
    service.shutdown().unwrap();
    drop(service);
    owner.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_view_paired_while_a_tab_works_hears_it_is_busy_at_once() {
    let scratch = crate::tests::scratch();
    let approval_data = crate::tests::scratch();
    let mut owner = approval_owner(approval_data.path()).await;
    let metadata = native_metadata::Fixture::new();
    let peer = PeerIdentity {
        id: "n-busy".into(),
        hostname: "offline".into(),
    };
    let (approvals, _store) = approve_peer(&owner, &metadata, &peer).await;
    let actor = approvals
        .authorize(&peer, wire::ClientId::new_v4(), 1)
        .unwrap();
    let mut service =
        Service::open_owned(scratch.path(), &std::env::current_exe().unwrap()).unwrap();
    let mut state = RemoteState::new(Default::default(), None, None);
    let created = service
        .host()
        .create_session(shellfox_core::CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: scratch.path().to_str().unwrap().into(),
            profile_id: Default::default(),
        })
        .unwrap();
    let tab = created.sessions[0].tabs[0].id;
    let row = |dot| crate::NativeTabActivity {
        tab,
        generation: shellfox_core::Generation::new(),
        dot,
        has_agents: true,
        closed: false,
        error: None,
        process_title: None,
        controlled: false,
    };
    // The agent was already working before this device connected.
    service.replace_tab_activity(vec![row(crate::activity::AgentState::Busy)]);
    dispatch(
        &mut service,
        &mut state,
        OwnerCommand::Pair {
            actor: actor.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(state.ready_views(&service).count(), 1, "due at once");
    let poll = OwnerCommand::Poll {
        actor: actor.clone(),
    };
    let polled = dispatch(&mut service, &mut state, poll.clone())
        .await
        .unwrap();
    let activity = |busy| serde_json::json!({"type":"terminal","event":{"type":"activity","tabId":tab,"busy":busy}});
    let native = serde_json::json!({"type":"changed","event":{"revision":created.revision,"reason":"native"}});
    assert_eq!(
        polled["events"],
        serde_json::json!([native, activity(true)])
    );
    assert_eq!(state.ready_views(&service).count(), 0);

    service.replace_tab_activity(vec![row(crate::activity::AgentState::Running)]);
    let polled = dispatch(&mut service, &mut state, poll).await.unwrap();
    assert_eq!(
        polled["events"],
        serde_json::json!([native, activity(false)])
    );
    service.shutdown().unwrap();
    drop(service);
    owner.shutdown().await.unwrap();
}

#[tokio::test]
async fn peer_requests_wait_behind_a_close_in_order_and_polls_do_not() {
    let scratch = crate::tests::scratch();
    let approval_data = crate::tests::scratch();
    let mut owner = approval_owner(approval_data.path()).await;
    let metadata = native_metadata::Fixture::new();
    let peer = PeerIdentity {
        id: "n-held".into(),
        hostname: "offline".into(),
    };
    let (approvals, _store) = approve_peer(&owner, &metadata, &peer).await;
    let actor = approvals
        .authorize(&peer, wire::ClientId::new_v4(), 1)
        .unwrap();
    let mut service =
        Service::open_owned(scratch.path(), &std::env::current_exe().unwrap()).unwrap();
    let mut state = RemoteState::new(Default::default(), None, None);
    let created = service
        .host()
        .create_session(shellfox_core::CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: scratch.path().to_str().unwrap().into(),
            profile_id: Default::default(),
        })
        .unwrap();
    let _outcome = service.park_close(created.sessions[0].tabs[0].id);
    let (sender, mut receiver) = shellfox_remote::owner_channel(4);
    let pair = sender.request(OwnerCommand::Pair {
        actor: actor.clone(),
    });
    let poll = sender.request(OwnerCommand::Poll {
        actor: actor.clone(),
    });
    let drive = async {
        let pair = receiver.recv().await.unwrap();
        state.dispatch_request(&mut service, pair);
        assert_eq!(state.held.0.len(), 1, "pairing waits for the close");
        let poll = receiver.recv().await.unwrap();
        state.dispatch_request(&mut service, poll);
        assert_eq!(state.held.0.len(), 1, "a poll is answered at once");
        service.abandon_close("test");
        while let Some(request) = state.next_held(&service) {
            state.dispatch_request(&mut service, request);
        }
    };
    let (paired, polled, ()) = tokio::join!(pair, poll, drive);
    // The poll ran before the view existed; the pairing ran after the close.
    assert_eq!(polled.unwrap_err().code, ErrorCode::AuthFailed);
    paired.unwrap();
    service.shutdown().unwrap();
    drop(service);
    owner.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_peer_revoked_during_its_close_commits_nothing() {
    let scratch = crate::tests::scratch();
    let approval_data = crate::tests::scratch();
    let mut owner = approval_owner(approval_data.path()).await;
    let metadata = native_metadata::Fixture::new();
    let peer = PeerIdentity {
        id: "n-revoked".into(),
        hostname: "offline".into(),
    };
    let (approvals, store) = approve_peer(&owner, &metadata, &peer).await;
    let actor = approvals
        .authorize(&peer, wire::ClientId::new_v4(), 1)
        .unwrap();
    let mut service =
        Service::open_owned(scratch.path(), &std::env::current_exe().unwrap()).unwrap();
    let mut state = RemoteState::new(Default::default(), None, None);
    // A failed launch leaves a runtime the close has to stop: the test binary
    // is no PTY supervisor.
    let created = service
        .host()
        .create_session(shellfox_core::CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: scratch.path().to_string_lossy().into_owned(),
            profile_id: shellfox_core::ProfileId::new("cmd").unwrap(),
        })
        .unwrap();
    let tab = created.sessions[0].tabs[0].id;
    assert!(service.attach(tab, 1, false).is_err());
    let generation = service.terminals[&tab].runtime.identity().generation;
    let command = RemoteCommand::CloseTab(wire::CloseTab {
        tab_id: tab,
        generation,
    });
    let (sender, mut receiver) = shellfox_remote::owner_channel(1);
    let close = sender.request(OwnerCommand::Execute {
        actor: actor.clone(),
        command: command.clone(),
    });
    let drive = async {
        let request = receiver.recv().await.unwrap();
        let mut bound = state.bind(&mut service);
        let target = crate::close::CloseTarget::Tab(tab);
        let kind = super::close::Kind::Tab {
            session: created.sessions[0].id,
        };
        bound.start_close(&actor, command, target, kind).unwrap();
        assert!(bound.keep_closing(request).is_none());
        approvals.revoke(&store, &peer.id).await.unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
        while service.metadata_busy() {
            assert!(std::time::Instant::now() < deadline, "close did not finish");
            service.pump();
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        state.finish_close(&mut service);
    };
    let (closed, ()) = tokio::join!(close, drive);
    assert_eq!(closed.unwrap_err().code, ErrorCode::AccessDenied);
    assert_eq!(
        service.host().snapshot().unwrap().sessions[0].tabs.len(),
        1,
        "metadata kept"
    );
    service.shutdown().unwrap();
    drop(service);
    owner.shutdown().await.unwrap();
}
