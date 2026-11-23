use super::{
    cache::{Grant, NativeState, View, ViewKey},
    *,
};
use shellfox_core::CreateSession;
use shellfox_runtime::{Chunk, Event, Identity, Lease, ObserverLease};

fn identity() -> Identity {
    Identity {
        session_id: SessionId::new(),
        tab_id: TabId::new(),
        generation: Generation::new(),
    }
}
fn frame(
    identity: Identity,
    sequence: u64,
    key: ViewKey,
    version: u64,
) -> Arc<shellfox_runtime::ViewFrame> {
    let mut terminal = shellfox_ghostty_vt::Terminal::new(4, 2).unwrap();
    let mut view = terminal.new_presentation_view().unwrap();
    let rendered = terminal.view_render(&mut view).unwrap();
    Arc::new(shellfox_runtime::ViewFrame {
        identity,
        sequence,
        version,
        view: if key.observer {
            shellfox_runtime::PresentationLease::Observer(ObserverLease {
                viewer: key.controller,
                epoch: 1,
            })
        } else {
            shellfox_runtime::PresentationLease::Controller(Lease {
                controller: key.controller,
                epoch: 1,
            })
        },
        screen: Arc::new(ScreenFrame {
            identity,
            sequence,
            screen: rendered.screen,
            mouse_tracking: false,
        }),
        row_metadata: rendered.row_metadata.into(),
        selection: terminal.view_selection(&mut view).unwrap(),
        cursor_visual: terminal.cursor_visual().unwrap(),
        top_row: rendered.top_row,
        total_rows: rendered.total_rows,
    })
}
fn attachment(id: Identity, controller: u64) -> Attachment {
    Attachment {
        controller,
        epoch: 1,
        generation: id.generation,
    }
}
fn key(id: Identity, controller: u64, observer: bool) -> ViewKey {
    ViewKey {
        controller,
        tab: id.tab_id,
        observer,
    }
}
fn scratch() -> tempfile::TempDir {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root).unwrap();
    tempfile::Builder::new()
        .prefix("native-service-test-")
        .tempdir_in(root)
        .unwrap()
}

#[test]
fn public_native_messages_cross_owner_thread_without_gui_types() {
    fn send<T: Send>() {}
    fn shared<T: Send + Sync>() {}
    send::<NativeAction>();
    send::<NativeResponse>();
    shared::<NativeFrame>();
    shared::<crate::OwnerClient>();
}

#[test]
fn latest_frame_is_shared_but_never_acknowledged_by_offer() {
    let id = identity();
    let mut state = NativeState::default();
    let a = attachment(id, 1);
    let control = key(id, 1, false);
    let observer = key(id, 2, true);
    state
        .views
        .insert(control, View::new(Grant::Control(a), 0, Phase::Active));
    state.views.insert(
        observer,
        View::new(
            Grant::Observer(NativeObserverAttachment {
                viewer: 2,
                epoch: 1,
                generation: id.generation,
            }),
            0,
            Phase::Active,
        ),
    );
    state.capture(control, frame(id, 3, control, 1)).unwrap();
    state.capture(observer, frame(id, 3, observer, 1)).unwrap();
    let first = state.offer(control).unwrap().unwrap();
    let cloned = state.offer(control).unwrap().unwrap();
    let second = state.offer(observer).unwrap().unwrap();
    assert!(Arc::ptr_eq(&first.presentation, &cloned.presentation));
    assert_ne!(first.presentation.view, second.presentation.view);
    assert!(state.views[&control].accepted.is_none());
    assert!(state.views[&observer].acked.is_none());
    assert!(state
        .views
        .get_mut(&control)
        .unwrap()
        .accepted_sequence(first.frame_id + 1)
        .is_err());
}

#[test]
fn acceptance_never_exceeds_delivered_and_recovery_barrier_is_first() {
    let id = identity();
    let mut view = View::new(Grant::Control(attachment(id, 1)), 5, Phase::Active);
    view.offered = Some((1, 10));
    assert_eq!(view.accepted_sequence(1).unwrap(), 5);
    view.barrier = None;
    view.delivered = 8;
    assert_eq!(view.accepted_sequence(1).unwrap(), 8);
    view.replay_lost = true;
    assert!(view.accepted_sequence(1).is_err());
}

#[test]
fn selection_repaint_has_a_new_frame_id_at_the_same_output_sequence() {
    let id = identity();
    let key = key(id, 1, false);
    let mut state = NativeState::default();
    state.views.insert(
        key,
        View::new(Grant::Control(attachment(id, 1)), 0, Phase::Active),
    );
    state.capture(key, frame(id, 7, key, 1)).unwrap();
    let first = state.offer(key).unwrap().unwrap();
    state.capture(key, frame(id, 7, key, 2)).unwrap();
    let next = state.offer(key).unwrap().unwrap();
    assert_ne!(first.frame_id, next.frame_id);
    assert_eq!(first.frame.sequence, next.frame.sequence);
    assert!(state
        .views
        .get_mut(&key)
        .unwrap()
        .accepted_sequence(first.frame_id)
        .is_err());
    assert_eq!(first.frame.screen.cells.len(), 8); // Old immutable Arc stays valid.
}

#[test]
fn output_facts_are_isolated_by_control_and_observer_epochs() {
    let id = identity();
    let control = key(id, 1, false);
    let observer = key(id, 2, true);
    let mut state = NativeState::default();
    state.views.insert(
        control,
        View::new(Grant::Control(attachment(id, 1)), 0, Phase::Active),
    );
    state.views.insert(
        observer,
        View::new(
            Grant::Observer(NativeObserverAttachment {
                viewer: 2,
                epoch: 3,
                generation: id.generation,
            }),
            0,
            Phase::Active,
        ),
    );
    state.record_event(
        id.tab_id,
        &Event::Output {
            lease: Lease {
                controller: 1,
                epoch: 2,
            },
            chunk: Chunk {
                sequence: 99,
                bytes: vec![],
            },
        },
    );
    assert_eq!(state.views[&control].delivered, 0);
    state.record_event(
        id.tab_id,
        &Event::ObserverOutput {
            observer: ObserverLease {
                viewer: 2,
                epoch: 3,
            },
            chunk: Chunk {
                sequence: 4,
                bytes: vec![],
            },
        },
    );
    assert_eq!(state.views[&observer].delivered, 4);
    assert_eq!(state.views[&control].delivered, 0);
}

#[test]
fn cache_and_views_are_bounded_and_stale_generations_fail_closed() {
    let mut state = NativeState::default();
    for _ in 0..128 {
        let id = identity();
        let k = key(id, 1, false);
        state.capture(k, frame(id, 0, k, 1)).unwrap();
    }
    let id = identity();
    let k = key(id, 1, false);
    assert!(state.capture(k, frame(id, 0, k, 1)).is_err());
    for _ in 0..128 {
        let id = identity();
        state.views.insert(
            key(id, 1, false),
            View::new(Grant::Control(attachment(id, 1)), 0, Phase::Active),
        );
    }
    assert!(state.admit(key(identity(), 1, false)).is_err());
    state.prune();
    assert!(state.offer(key(identity(), 1, false)).is_err());
}

#[test]
fn native_metadata_uses_real_core_snapshot_and_safe_close_path() {
    let data = scratch();
    let mut service =
        crate::Service::open_owned(data.path(), &std::env::current_exe().unwrap()).unwrap();
    let input = CreateSession {
        request_id: SessionId::new(),
        title: Some("native fixture".into()),
        cwd: data.path().to_string_lossy().into_owned(),
        profile_id: crate::tests::default_catalog_profile(),
    };
    let NativeResponse::Snapshot(created) = service
        .execute_native(NativeAction::Metadata(HostRequest::CreateSession(input)), 1)
        .unwrap()
    else {
        panic!("snapshot")
    };
    let session = created.sessions[0].id;
    let tab = created.sessions[0].tabs[0].id;
    let NativeResponse::Status(status) = service.execute_native(NativeAction::Status, 1).unwrap()
    else {
        panic!("status")
    };
    assert_eq!(status.active_terminals, 0); // One metadata tab is not a live owner.
    assert!(service
        .execute_native(
            NativeAction::Metadata(HostRequest::RenameSession {
                session_id: session,
                title: "\0".into()
            }),
            1
        )
        .is_err());
    assert!(service
        .execute_native(NativeAction::Observe { tab }, 2)
        .is_err()); // No implicit PTY for read-only view.
    let NativeResponse::Snapshot(closed) = service
        .execute_native(NativeAction::CloseTab { tab }, 1)
        .unwrap()
    else {
        panic!("closed")
    };
    assert!(closed.sessions[0].tabs.is_empty());
    assert_eq!(service.active_terminals(), 0);
    service.shutdown().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn owner_clients_share_the_same_owner_and_metadata() {
    let data = scratch();
    let mut owner = crate::owner::ServiceOwner::start(
        data.path().to_owned(),
        std::env::current_exe().unwrap(),
        shellfox_protocol::CancellationToken::new(),
        (),
    )
    .await
    .unwrap();
    let first = owner.client(uuid::Uuid::new_v4());
    let second = owner.client(uuid::Uuid::new_v4());
    let input = CreateSession {
        request_id: SessionId::new(),
        title: Some("shared owner".into()),
        cwd: data.path().to_string_lossy().into_owned(),
        profile_id: crate::tests::default_catalog_profile(),
    };
    first
        .native(NativeAction::Metadata(HostRequest::CreateSession(input)))
        .await
        .unwrap();
    let NativeResponse::Snapshot(snapshot) = second
        .native(NativeAction::Metadata(HostRequest::GetSnapshot {}))
        .await
        .unwrap()
    else {
        panic!("snapshot")
    };
    assert_eq!(snapshot.sessions.len(), 1);
    first.native(NativeAction::DetachAll).await.unwrap();
    let NativeResponse::Snapshot(snapshot) = second
        .native(NativeAction::Metadata(HostRequest::GetSnapshot {}))
        .await
        .unwrap()
    else {
        panic!("snapshot")
    };
    assert_eq!(snapshot.sessions.len(), 1);
    owner.shutdown().await.unwrap();
}
