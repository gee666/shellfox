//! Last opener wins, and control comes back, as in 0.9: a phone that opens a
//! terminal takes it from the desktop; when the phone leaves (locked, killed,
//! expired), the views still showing the terminal hear it is free and take it
//! back without a click.
use super::tests::{approval_owner, approve_peer, dispatch};
use super::*;
use crate::native_metadata_fixture as native_metadata;
use shellfox_transport::incoming::PeerIdentity;

fn execute(actor: &Actor, command: RemoteCommand) -> OwnerCommand {
    OwnerCommand::Execute {
        actor: actor.clone(),
        command,
    }
}

/// The desktop's tab-activity row for `tab` after the owner pumps, as its
/// held feed would read it. Facts of a just-started shell may take a moment.
fn desktop_row(service: &mut Service, tab: shellfox_core::TabId) -> crate::NativeTabActivity {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        service.pump();
        let row = service
            .published_tab_activity()
            .iter()
            .find(|row| row.tab == tab)
            .cloned();
        match row {
            Some(row) => return row,
            None if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(50))
            }
            None => panic!("a running tab has an activity row"),
        }
    }
}

fn control_events(polled: &serde_json::Value) -> Vec<(String, u64)> {
    polled["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["type"] == "terminal" && event["event"]["type"] == "control")
        .map(|event| {
            (
                event["event"]["control"].as_str().unwrap().to_owned(),
                event["event"]["controlEpoch"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
#[ignore = "run through `pnpm test:remote:loopback`: needs the built CLI as PTY supervisor"]
async fn control_returns_to_the_remaining_views_when_the_phone_leaves() {
    let supervisor = std::path::PathBuf::from(
        std::env::var_os("SHELLFOX_TEST_SUPERVISOR").expect("compiled CLI path"),
    );
    let scratch = crate::tests::scratch();
    let approval_data = crate::tests::scratch();
    let mut owner = approval_owner(approval_data.path()).await;
    let metadata = native_metadata::Fixture::new();
    let peer = PeerIdentity {
        id: "n-control".into(),
        hostname: "fixture".into(),
    };
    let (approvals, _store) = approve_peer(&owner, &metadata, &peer).await;
    let phone = approvals
        .authorize(&peer, wire::ClientId::new_v4(), 1)
        .unwrap();
    let watcher = approvals
        .authorize(&peer, wire::ClientId::new_v4(), 1)
        .unwrap();
    let mut service = Service::open_owned(scratch.path(), &supervisor).unwrap();
    let mut state = RemoteState::new(Default::default(), None, None);
    let created = service
        .host()
        .create_session(shellfox_core::CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: scratch.path().to_str().unwrap().into(),
            profile_id: crate::tests::default_catalog_profile(),
        })
        .unwrap();
    let tab = created.sessions[0].tabs[0].id;

    // View A, the desktop, shows the tab and controls it.
    let desktop = service.attach(tab, 1, false).unwrap();
    assert!(service.controlled(tab));
    for actor in [&phone, &watcher] {
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
    // A second remote view, such as a peer desktop, waits behind the desktop.
    let blocked = dispatch(
        &mut service,
        &mut state,
        execute(
            &watcher,
            RemoteCommand::AttachTerminal(wire::AttachTerminal {
                tab_id: tab,
                after_sequence: None,
                generation: None,
            }),
        ),
    )
    .await
    .unwrap();
    assert_eq!(blocked["control"], "blocked");
    let blocked_epoch = blocked["controlEpoch"].as_u64().unwrap();

    let take = |actor: &Actor| {
        execute(
            actor,
            RemoteCommand::TakeTerminalControl(wire::TakeTerminalControl {
                tab_id: tab,
                generation: desktop.generation,
            }),
        )
    };
    // View B, the phone, opens the terminal and takes it.
    dispatch(&mut service, &mut state, take(&phone))
        .await
        .unwrap();
    assert!(
        service.lease(tab, &desktop).is_err(),
        "the desktop lost control"
    );
    assert!(desktop_row(&mut service, tab).controlled);
    let poll = OwnerCommand::Poll {
        actor: watcher.clone(),
    };
    let polled = dispatch(&mut service, &mut state, poll.clone())
        .await
        .unwrap();
    assert!(
        control_events(&polled).is_empty(),
        "still controlled elsewhere"
    );

    // The phone locks: its client detaches the view.
    dispatch(
        &mut service,
        &mut state,
        execute(
            &phone,
            RemoteCommand::DetachTerminal(wire::DetachTerminal {
                tab_id: tab,
                generation: desktop.generation,
            }),
        ),
    )
    .await
    .unwrap();
    assert!(!service.controlled(tab));
    // The desktop hears it from its held activity feed...
    assert!(
        !desktop_row(&mut service, tab).controlled,
        "the desktop's feed reports control free"
    );
    // ...and the waiting remote view from an owned control event, once.
    assert!(state.views[&key(&watcher)].has_events(&service, service.host().revision()));
    let polled = dispatch(&mut service, &mut state, poll.clone())
        .await
        .unwrap();
    let events = control_events(&polled);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "owned");
    assert!(events[0].1 > blocked_epoch);
    let polled = dispatch(&mut service, &mut state, poll.clone())
        .await
        .unwrap();
    assert!(control_events(&polled).is_empty(), "told once");

    // The desktop takes control back without a takeover.
    let desktop = service.attach(tab, 1, false).unwrap();
    assert!(service.controlled(tab));

    // The phone comes back and takes control again.
    dispatch(&mut service, &mut state, take(&phone))
        .await
        .unwrap();
    assert!(service.lease(tab, &desktop).is_err());
    // Its view expires (killed or offline for 60 s): the backend disconnects it.
    dispatch(
        &mut service,
        &mut state,
        OwnerCommand::Disconnect {
            actor: phone.clone(),
        },
    )
    .await
    .unwrap();
    assert!(!service.controlled(tab));
    let desktop = service.attach(tab, 1, false).unwrap();
    assert!(service.lease(tab, &desktop).is_ok());

    service.shutdown().unwrap();
    drop(service);
    owner.shutdown().await.unwrap();
}
