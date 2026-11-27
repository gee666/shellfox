//! A published terminal title alone reaches a paired remote view: its next
//! poll carries a "native" change, and the snapshot it then reads names the tab.
use super::tests::{approval_owner, approve_peer, dispatch};
use super::*;
use crate::activity::integration::{read_source, StartupEvidence, TerminalSourceFacts};
use crate::native_metadata_fixture as native_metadata;
use shellfox_runtime::{Identity, Phase, SourceKind};
use shellfox_transport::incoming::PeerIdentity;

/// Publishes owner facts for one running tab whose program set `title`, as
/// the owner pump does, without launching a shell.
fn publish(service: &mut Service, identity: Identity, title: &str) {
    let snapshot = service.host().snapshot().unwrap();
    let settings = service.host().global_settings().unwrap();
    service
        .publish_source_facts(snapshot.clone(), settings.clone())
        .unwrap();
    let creation = read_source(&service.source_facts_watch())
        .unwrap()
        .creation_facts
        .clone();
    let terminal = TerminalSourceFacts {
        identity,
        phase: Phase::Active,
        source_kind: Some(SourceKind::OwnedPty),
        automation_exit_code: None,
        startup: StartupEvidence::Confirmed,
        observation: None,
        busy: false,
        activity_available: false,
        title: Some(title.into()),
    };
    let cancelled = service.ownership_lost.clone();
    service
        .activity
        .publisher
        .publish_with_creation_facts(snapshot, settings, vec![terminal], creation, cancelled)
        .unwrap();
    service.publish_native_watches();
}

fn snapshot_title(service: &Service) -> String {
    let source = read_source(&service.source_facts_watch()).unwrap();
    let session = &source.snapshot.sessions[0];
    let facts = crate::facts::projection::project_session_facts(&source).unwrap();
    let dto = shellfox_remote::adapter::session_dto(session, &facts[&session.id]).unwrap();
    dto.tabs[0].title.clone()
}

#[tokio::test]
async fn a_title_change_alone_tells_a_remote_view_to_read_the_snapshot() {
    let scratch = crate::tests::scratch();
    let approval_data = crate::tests::scratch();
    let mut owner = approval_owner(approval_data.path()).await;
    let metadata = native_metadata::Fixture::new();
    let peer = PeerIdentity {
        id: "n-title".into(),
        hostname: "offline".into(),
    };
    let (approvals, _store) = approve_peer(&owner, &metadata, &peer).await;
    let actor = approvals
        .authorize(&peer, wire::ClientId::new_v4(), 1)
        .unwrap();
    let mut service =
        Service::open_owned(scratch.path(), &std::env::current_exe().unwrap()).unwrap();
    let created = service
        .create_application_session(shellfox_core::CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: scratch.path().to_str().unwrap().into(),
            profile_id: crate::tests::default_catalog_profile(),
        })
        .unwrap();
    let tab = &created.sessions[0].tabs[0];
    assert!(tab.use_process_title);
    let identity = Identity {
        session_id: tab.session_id,
        tab_id: tab.id,
        generation: shellfox_core::Generation::new(),
    };
    publish(&mut service, identity, "Agent at work");
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
    let poll = OwnerCommand::Poll {
        actor: actor.clone(),
    };
    dispatch(&mut service, &mut state, poll.clone())
        .await
        .unwrap();
    assert_eq!(state.ready_views(&service).count(), 0);
    let revision = service.host().revision();

    // Only the title changes: no metadata revision, yet the view is woken.
    publish(&mut service, identity, "Agent done");
    assert_eq!(service.host().revision(), revision);
    assert_eq!(state.ready_views(&service).count(), 1);
    let polled = dispatch(&mut service, &mut state, poll.clone())
        .await
        .unwrap();
    let native =
        serde_json::json!({"type":"changed","event":{"revision":revision,"reason":"native"}});
    assert_eq!(polled["events"], serde_json::json!([native]));
    assert_eq!(snapshot_title(&service), "Agent done");
    // The same title again is not a change.
    publish(&mut service, identity, "Agent done");
    assert_eq!(state.ready_views(&service).count(), 0);

    // After a rename, published titles neither name the tab nor wake the view.
    service.host().rename_tab(tab.id, "Mine").unwrap();
    publish(&mut service, identity, "Agent again");
    dispatch(&mut service, &mut state, poll).await.unwrap();
    publish(&mut service, identity, "Agent once more");
    assert_eq!(state.ready_views(&service).count(), 0);
    assert_eq!(snapshot_title(&service), "Mine");
    service.shutdown().unwrap();
    drop(service);
    owner.shutdown().await.unwrap();
}
