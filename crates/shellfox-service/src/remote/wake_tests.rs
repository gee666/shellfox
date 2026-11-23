use super::tests::{approval_owner, approve_peer, dispatch};
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
