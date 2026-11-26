//! A peer's change answers once it is on disk; the owner never waits for it.
use super::{tests::*, *};
use crate::native_metadata_fixture as native_metadata;
use shellfox_remote::{
    adapter::{ObservationFacts, SessionFacts, TabFacts},
    owner_channel, RemoteHost,
};
use shellfox_transport::incoming::PeerIdentity;
use std::time::Duration;

#[tokio::test]
async fn a_peer_rename_answers_once_the_disk_has_it() {
    let scratch = crate::tests::scratch();
    let approval_data = crate::tests::scratch();
    let mut owner = approval_owner(approval_data.path()).await;
    let metadata = native_metadata::Fixture::new();
    let peer = PeerIdentity {
        id: "n-stalled-disk".into(),
        hostname: "offline".into(),
    };
    let (approvals, _store) = approve_peer(&owner, &metadata, &peer).await;
    let actor = approvals
        .authorize(&peer, wire::ClientId::new_v4(), 1)
        .unwrap();
    let mut service =
        Service::open_owned(scratch.path(), &std::env::current_exe().unwrap()).unwrap();
    service.runtime_handle = Some(tokio::runtime::Handle::current());
    service
        .host()
        .start_journal(service.durability.hooks())
        .unwrap();
    let created = service
        .host()
        .create_session(shellfox_core::CreateSession {
            request_id: SessionId::new(),
            title: Some("Before".into()),
            cwd: scratch.path().to_str().unwrap().into(),
            profile_id: Default::default(),
        })
        .unwrap();
    let session = &created.sessions[0];
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
    state.facts.sessions.insert(
        session.id,
        SessionFacts {
            adapter_id: wire::AdapterId::EmbeddedPty,
            shell_id: wire::ShellId::LoginShell,
            window: None,
            can_focus: false,
            can_add_tab: false,
            control_reason: Some("No runtime in this fixture".into()),
            error: None,
            terminal_lifetime: Some(wire::TerminalLifetime::AppOwned),
            shell_survival: Some(false),
            tabs: [(
                session.tabs[0].id,
                TabFacts {
                    created_at: session.tabs[0].created_at.clone().unwrap(),
                    lifecycle: wire::TabLifecycle::Closed,
                    observation: ObservationFacts::Unavailable {
                        reason: "No runtime launched".into(),
                    },
                    error: None,
                    generation: None,
                    terminal_kind: Some(wire::TerminalKind::Embedded),
                    exit_code: Some(None),
                },
            )]
            .into(),
        },
    );
    service.host().flush(Duration::from_secs(10)).unwrap();
    let store = scratch.path().join(shellfox_host::STORE_FILENAME);
    rusqlite::Connection::open(&store)
        .unwrap()
        .execute_batch("CREATE TRIGGER stalled BEFORE UPDATE ON host_state BEGIN SELECT RAISE(ABORT,'disk stalled'); END;")
        .unwrap();
    let (sender, mut receiver) = owner_channel(1);
    let rename = OwnerCommand::Execute {
        actor: actor.clone(),
        command: RemoteCommand::RenameSession(wire::RenameSession {
            session_id: session.id,
            title: "After".into(),
        }),
    };
    let reply = tokio::spawn(async move { sender.request(rename).await });
    let request = receiver.recv().await.unwrap();
    state.dispatch_request(&mut service, request);
    // The owner moved on at once; the peer's answer waits for the disk.
    assert_eq!(
        service.host().snapshot().unwrap().sessions[0].title,
        "After"
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!reply.is_finished());
    rusqlite::Connection::open(&store)
        .unwrap()
        .execute_batch("DROP TRIGGER stalled")
        .unwrap();
    let response = tokio::time::timeout(Duration::from_secs(10), reply)
        .await
        .expect("the peer's answer arrives once the change is saved")
        .unwrap()
        .unwrap();
    let dto: wire::SessionDto = serde_json::from_value(response).unwrap();
    assert_eq!(dto.title, "After");
    drop(service);
    owner.shutdown().await.unwrap();
}
