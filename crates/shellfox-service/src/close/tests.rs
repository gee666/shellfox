use super::*;
use crate::tests::{default_catalog_profile, scratch};
use shellfox_core::{AddTab, CreateSession};

#[test]
fn a_close_in_flight_lets_reads_through_and_holds_metadata_writes() {
    let data = scratch();
    let mut service = Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    // A failed launch still registers a runtime that the close must stop.
    let cwd = data
        .path()
        .join("missing-cwd")
        .to_string_lossy()
        .into_owned();
    let profile_id = shellfox_core::ProfileId::new("cmd").unwrap();
    let created = service
        .host()
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd,
            profile_id: profile_id.clone(),
        })
        .unwrap();
    let session = created.sessions[0].id;
    let (first, second) = {
        let snapshot = service
            .host()
            .add_tab(
                session,
                AddTab {
                    title: None,
                    cwd: None,
                    profile_id,
                },
            )
            .unwrap();
        (
            snapshot.sessions[0].tabs[0].id,
            snapshot.sessions[0].tabs[1].id,
        )
    };
    assert!(service.attach(first, 1, false).is_err());
    assert!(service.attach(second, 1, false).is_err());
    assert_eq!(service.active_terminals(), 2);

    // While a close waits for its runtime: reads and other tabs' I/O run.
    let _parked = service.park_close(first);
    {
        assert!(service.tab_closing(first) && !service.tab_closing(second));
        let attachment = crate::Attachment {
            controller: 1,
            epoch: 1,
            generation: Default::default(),
        };
        let read = |tab| LocalApiRequest::Poll {
            tab,
            attachment: attachment.clone(),
        };
        assert!(service.local_api_flows(&LocalApiRequest::Snapshot {}));
        assert!(
            service.local_api_flows(&read(second)),
            "I/O on another tab runs"
        );
        assert!(
            !service.local_api_flows(&read(first)),
            "the closing tab waits"
        );
        let rename = LocalApiRequest::RenameSession {
            id: session,
            title: "later".into(),
        };
        assert!(!service.local_api_flows(&rename), "metadata writes wait");
        assert!(service.host().snapshot().is_ok());
        // A second close can't start while the first holds its token.
        assert!(service.close_tab(second).is_err());
    }
    service.abandon_close("test");
    let (send, outcome) = std::sync::mpsc::channel();
    service.begin_close(
        CloseTarget::Tab(first),
        Box::new(move |result| {
            let _ = send.send(result);
        }),
    );
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
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd,
            profile_id: default_catalog_profile(),
        })
        .unwrap();
    let tab = created.sessions[0].tabs[0].id;
    // No live terminal: the close would finish on its first pass, so park it.
    let outcome = service.park_close(tab);
    service.shutdown().unwrap();
    assert!(outcome.try_recv().unwrap().is_err());
    assert_eq!(service.host().snapshot().unwrap().sessions[0].tabs.len(), 1);
}

/// Tabs whose close panics in `start_close`, for the owner's panic test.
static PANIC_ON_CLOSE: std::sync::Mutex<Vec<TabId>> = std::sync::Mutex::new(Vec::new());
pub(super) fn panic_if_marked(target: CloseTarget) {
    if let CloseTarget::Tab(tab) = target {
        if PANIC_ON_CLOSE.lock().unwrap().contains(&tab) {
            panic!("test close panic");
        }
    }
}

async fn owner_with<D: crate::OwnerDispatcher + 'static>(
    data: &std::path::Path,
    dispatcher: D,
) -> crate::owner::ServiceOwner {
    let owner = crate::owner::ServiceOwner::start(
        data.to_owned(),
        std::env::current_exe().unwrap(),
        shellfox_protocol::CancellationToken::new(),
        dispatcher,
    )
    .await
    .unwrap();
    owner.activate().await.unwrap();
    owner
}
async fn session(client: &crate::OwnerClient, data: &std::path::Path) -> (SessionId, TabId) {
    let crate::local_api::LocalApiResponse::Snapshot(created) = client
        .execute(LocalApiRequest::CreateSession {
            input: CreateSession {
                request_id: SessionId::new(),
                title: Some("Original".into()),
                cwd: data.to_string_lossy().into_owned(),
                profile_id: default_catalog_profile(),
            },
        })
        .await
        .unwrap()
    else {
        panic!("snapshot")
    };
    (created.sessions[0].id, created.sessions[0].tabs[0].id)
}

#[tokio::test]
async fn a_panic_inside_a_close_still_runs_owner_cleanup() {
    let data = scratch();
    let mut owner = owner_with(data.path(), ()).await;
    let client = owner.client(uuid::Uuid::new_v4());
    let (_, tab) = session(&client, data.path()).await;
    PANIC_ON_CLOSE.lock().unwrap().push(tab);
    let closed = client.execute(LocalApiRequest::CloseTab { id: tab }).await;
    assert!(closed.is_err(), "the panicked close gets no snapshot");
    // Cleanup ran: shutdown is confirmed and the metadata lock is released.
    owner.shutdown().await.unwrap();
    let host = shellfox_host::Host::open(data.path()).unwrap();
    assert_eq!(
        host.snapshot().unwrap().sessions[0].tabs.len(),
        1,
        "nothing committed"
    );
}

/// Parks a close on the first pass after a tab is named.
type Outcome = std::sync::mpsc::Receiver<Result<Snapshot>>;
struct Parker {
    tab: std::sync::Arc<std::sync::Mutex<Option<TabId>>>,
    outcome: std::sync::Arc<std::sync::Mutex<Option<Outcome>>>,
}
impl crate::OwnerDispatcher for Parker {
    fn dispatch_one(&mut self, service: &mut Service) {
        if let Some(tab) = self.tab.lock().unwrap().take() {
            *self.outcome.lock().unwrap() = Some(service.park_close(tab));
        }
    }
}

#[tokio::test]
async fn shutdown_answers_requests_held_behind_a_close_and_commits_none() {
    use std::sync::Arc;
    let data = scratch();
    let tab = Arc::new(std::sync::Mutex::new(None));
    let outcome = Arc::new(std::sync::Mutex::new(None));
    let mut owner = owner_with(
        data.path(),
        Parker {
            tab: tab.clone(),
            outcome: outcome.clone(),
        },
    )
    .await;
    let client = owner.client(uuid::Uuid::new_v4());
    let (session, closing) = session(&client, data.path()).await;
    *tab.lock().unwrap() = Some(closing);
    while outcome.lock().unwrap().is_none() {
        client.execute(LocalApiRequest::Snapshot {}).await.unwrap();
    }
    let rename = client
        .try_execute(LocalApiRequest::RenameSession {
            id: session,
            title: "Held".into(),
        })
        .unwrap();
    // The inbox is FIFO: once this read is answered, the rename has been held.
    client.execute(LocalApiRequest::Snapshot {}).await.unwrap();
    owner.shutdown().await.unwrap();
    let close = outcome.lock().unwrap().take().unwrap().try_recv();
    assert!(
        matches!(close, Ok(Err(_))),
        "shutdown fails the close explicitly"
    );
    let held = tokio::time::timeout(Duration::from_secs(5), rename).await;
    assert!(
        matches!(held, Ok(Err(_)) | Ok(Ok(Err(_)))),
        "the held request is answered, not committed"
    );
    let host = shellfox_host::Host::open(data.path()).unwrap();
    let snapshot = host.snapshot().unwrap();
    assert_eq!(snapshot.sessions[0].title, "Original");
    assert_eq!(
        snapshot.sessions[0].tabs.len(),
        1,
        "the close was not committed"
    );
}

#[tokio::test]
async fn an_attach_routed_through_a_start_reports_a_failed_start() {
    let data = scratch();
    let mut owner = owner_with(data.path(), ()).await;
    let client = owner.client(uuid::Uuid::new_v4());
    let crate::local_api::LocalApiResponse::Snapshot(created) = client
        .execute(LocalApiRequest::CreateSession {
            input: CreateSession {
                request_id: SessionId::new(),
                title: None,
                cwd: data
                    .path()
                    .join("missing-cwd")
                    .to_string_lossy()
                    .into_owned(),
                profile_id: shellfox_core::ProfileId::new("cmd").unwrap(),
            },
        })
        .await
        .unwrap()
    else {
        panic!("snapshot")
    };
    let tab = created.sessions[0].tabs[0].id;
    assert!(client
        .execute(LocalApiRequest::Attach {
            tab,
            takeover: false
        })
        .await
        .is_err());
    // Reported once; the failed owner stays registered until a close confirms it.
    client
        .execute(LocalApiRequest::CloseTab { id: tab })
        .await
        .unwrap();
    owner.shutdown().await.unwrap();
}
