use super::*;
use crate::tests::{default_catalog_profile, scratch};
use shellfox_core::{AddTab, CreateSession};

#[test]
fn a_close_in_flight_lets_reads_through_and_holds_metadata_writes() {
    let data = scratch();
    let mut service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    // A failed launch still registers a runtime that the close must stop.
    let cwd = data.path().join("missing-cwd").to_string_lossy().into_owned();
    let profile_id = shellfox_core::ProfileId::new("cmd").unwrap();
    let created = service
        .host()
        .create_session(CreateSession { request_id: SessionId::new(), title: None, cwd, profile_id: profile_id.clone() })
        .unwrap();
    let session = created.sessions[0].id;
    let (first, second) = {
        let snapshot = service.host().add_tab(session, AddTab { title: None, cwd: None, profile_id }).unwrap();
        (snapshot.sessions[0].tabs[0].id, snapshot.sessions[0].tabs[1].id)
    };
    assert!(service.attach(first, 1, false).is_err());
    assert!(service.attach(second, 1, false).is_err());
    assert_eq!(service.active_terminals(), 2);

    // While a close waits for its runtime: reads and other tabs' I/O run.
    let _parked = service.park_close(first);
    {
        assert!(service.tab_closing(first) && !service.tab_closing(second));
        let attachment = crate::Attachment { controller: 1, epoch: 1, generation: Default::default() };
        let read = |tab| LocalApiRequest::Poll { tab, attachment: attachment.clone() };
        assert!(service.local_api_flows(&LocalApiRequest::Snapshot {}));
        assert!(service.local_api_flows(&read(second)), "I/O on another tab runs");
        assert!(!service.local_api_flows(&read(first)), "the closing tab waits");
        let rename = LocalApiRequest::RenameSession { id: session, title: "later".into() };
        assert!(!service.local_api_flows(&rename), "metadata writes wait");
        assert!(service.host().snapshot().is_ok());
        // A second close can't start while the first holds its token.
        assert!(service.close_tab(second).is_err());
    }
    service.abandon_close("test");
    let (send, outcome) = std::sync::mpsc::channel();
    service.begin_close(CloseTarget::Tab(first), Box::new(move |result| {
        let _ = send.send(result);
    }));
    let deadline = Instant::now() + STOP_DEADLINE;
    let closed = loop {
        if let Ok(result) = outcome.try_recv() {
            break result.unwrap();
        }
        assert!(Instant::now() < deadline, "close did not finish");
        service.pump();
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(closed.sessions[0].tabs.len(), 1);
    assert!(!service.metadata_busy() && service.terminals.contains_key(&second));
    service.close_tab(second).unwrap();
    service.shutdown().unwrap();
}

#[test]
fn shutdown_fails_a_close_in_flight_and_keeps_its_metadata() {
    let data = scratch();
    let mut service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    let cwd = data.path().to_string_lossy().into_owned();
    let created = service
        .host()
        .create_session(CreateSession { request_id: SessionId::new(), title: None, cwd, profile_id: default_catalog_profile() })
        .unwrap();
    let tab = created.sessions[0].tabs[0].id;
    // No live terminal: the close would finish on its first pass, so park it.
    let outcome = service.park_close(tab);
    service.shutdown().unwrap();
    assert!(outcome.try_recv().unwrap().is_err());
    assert_eq!(service.host().snapshot().unwrap().sessions[0].tabs.len(), 1);
}

