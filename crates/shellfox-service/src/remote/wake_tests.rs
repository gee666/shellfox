use super::tests::{approval_owner, approve_peer, dispatch};
use shellfox_remote::RemoteHost;
use super::*;
use crate::native_metadata_fixture as native_metadata;
use shellfox_transport::incoming::PeerIdentity;

#[tokio::test]
async fn host_revision_wakes_every_view_until_it_polls() {
    let scratch = crate::tests::scratch();
    let approval_data = crate::tests::scratch();
    let mut owner = approval_owner(approval_data.path()).await;
    let metadata = native_metadata::Fixture::new();
    let peer = PeerIdentity { id: "n-wake".into(), hostname: "offline".into() };
    let (approvals, _store) = approve_peer(&owner, &metadata, &peer).await;
    let first = approvals.authorize(&peer, wire::ClientId::new_v4(), 1).unwrap();
    let second = approvals.authorize(&peer, wire::ClientId::new_v4(), 1).unwrap();
    let mut service =
        Service::open_owned(scratch.path(), &std::env::current_exe().unwrap()).unwrap();
    let mut state = RemoteState::new(Default::default(), None, None);
    for actor in [&first, &second] {
        dispatch(&mut service, &mut state, OwnerCommand::Pair { actor: actor.clone() })
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
    let polled = dispatch(&mut service, &mut state, OwnerCommand::Poll { actor: first.clone() })
        .await
        .unwrap();
    assert_eq!(polled["events"].as_array().unwrap().len(), 1);
    let ready: Vec<_> = state.ready_views(&service).map(|view| view.controller).collect();
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
    let peer = PeerIdentity { id: "n-activity".into(), hostname: "offline".into() };
    let (approvals, _store) = approve_peer(&owner, &metadata, &peer).await;
    let actor = approvals.authorize(&peer, wire::ClientId::new_v4(), 1).unwrap();
    let mut service =
        Service::open_owned(scratch.path(), &std::env::current_exe().unwrap()).unwrap();
    let mut state = RemoteState::new(Default::default(), None, None);
    dispatch(&mut service, &mut state, OwnerCommand::Pair { actor: actor.clone() })
        .await
        .unwrap();
    assert_eq!(state.ready_views(&service).count(), 0, "pairing starts at the current activity");
    service.replace_tab_activity(Vec::new());
    assert_eq!(state.ready_views(&service).count(), 1);
    let polled = dispatch(&mut service, &mut state, OwnerCommand::Poll { actor: actor.clone() })
        .await
        .unwrap();
    assert_eq!(polled["events"], serde_json::json!([{"type":"changed","event":{"revision":0,"reason":"native"}}]));
    assert_eq!(state.ready_views(&service).count(), 0, "one event per change");
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
    let peer = PeerIdentity { id: "n-held".into(), hostname: "offline".into() };
    let (approvals, _store) = approve_peer(&owner, &metadata, &peer).await;
    let actor = approvals.authorize(&peer, wire::ClientId::new_v4(), 1).unwrap();
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
    let pair = sender.request(OwnerCommand::Pair { actor: actor.clone() });
    let poll = sender.request(OwnerCommand::Poll { actor: actor.clone() });
    let drive = async {
        let pair = receiver.recv().await.unwrap();
        state.dispatch_request(&mut service, pair);
        assert_eq!(state.held.0.len(), 1, "pairing waits for the close");
        let poll = receiver.recv().await.unwrap();
        state.dispatch_request(&mut service, poll);
        assert_eq!(state.held.0.len(), 1, "a poll is answered at once");
        service.abandon_close("test");
        state.release_held(&mut service);
        assert!(state.held.0.is_empty());
    };
    let (paired, polled, ()) = tokio::join!(pair, poll, drive);
    // The poll ran before the view existed; the pairing ran after the close.
    assert_eq!(polled.unwrap_err().code, ErrorCode::AuthFailed);
    paired.unwrap();
    service.shutdown().unwrap();
    drop(service);
    owner.shutdown().await.unwrap();
}
