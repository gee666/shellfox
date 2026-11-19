use super::*;
use shellfox_core::{Preferences, ProfileId, Tab};

fn session() -> Session {
    let id = SessionId::new();
    Session {
        id,
        title: "Window fixture".into(),
        cwd: "/fixture".into(),
        created_at: "2025-01-01T00:00:00Z".into(),
        updated_at: "2025-01-01T00:00:00Z".into(),
        settled_at: None,
        pinned_at: None,
        env: vec![],
        tabs: (0..3)
            .map(|ordinal| Tab {
                id: TabId::new(),
                session_id: id,
                title: format!("Tab {ordinal}"),
                use_process_title: false,
                cwd: "/fixture".into(),
                ordinal,
                created_at: None,
                profile_id: ProfileId::default(),
                wsl_launch: None,
                state: TabState::Pending,
            })
            .collect(),
    }
}
fn snapshot() -> Snapshot {
    let sessions = vec![session(), session()];
    let selection = Selection {
        session_id: Some(sessions[0].id),
        tab_id: Some(sessions[0].tabs[0].id),
    };
    Snapshot {
        revision: 1,
        sessions,
        preferences: Preferences::default(),
        selection,
    }
}
#[test]
fn another_windows_global_recent_never_changes_local_selection_or_epoch() {
    let mut snapshot = snapshot();
    let mut a = WindowSelection::new(InitialSelection::Recent);
    let mut b = WindowSelection::new(InitialSelection::Session(snapshot.sessions[1].id));
    a.accept_snapshot(&snapshot);
    b.accept_snapshot(&snapshot);
    let a_selection = a.selected().clone();
    let a_epoch = a.epoch();
    let b_tab = snapshot.sessions[1].tabs[2].id;
    assert!(b.select_tab(&snapshot, snapshot.sessions[1].id, b_tab));
    snapshot.selection = b.selected().clone();
    snapshot.revision += 1;
    a.accept_snapshot(&snapshot);
    b.accept_snapshot(&snapshot);
    assert_eq!(*a.selected(), a_selection);
    assert_eq!(a.epoch(), a_epoch);
    assert_eq!(b.selected().tab_id, Some(b_tab));
}
#[test]
fn explicit_new_window_session_uses_private_active_tabs_and_missing_saved_tab_is_blank() {
    let snapshot = snapshot();
    let first = &snapshot.sessions[0];
    let second = &snapshot.sessions[1];
    let mut selection = WindowSelection::new(InitialSelection::Session(second.id));
    selection.accept_snapshot(&snapshot);
    assert_eq!(selection.selected().session_id, Some(second.id));
    selection.select_tab(&snapshot, second.id, second.tabs[2].id);
    selection.select_session(&snapshot, first.id);
    selection.select_session(&snapshot, second.id);
    assert_eq!(selection.selected().tab_id, Some(second.tabs[2].id));
    let mut missing = WindowSelection::new(InitialSelection::Tab {
        session: first.id,
        tab: TabId::new(),
    });
    missing.accept_snapshot(&snapshot);
    assert_eq!(*missing.selected(), Selection::default());
    let mut empty = WindowSelection::new(InitialSelection::Empty);
    empty.accept_snapshot(&snapshot);
    assert_eq!(*empty.selected(), Selection::default());
}
#[test]
fn right_then_left_after_close_and_late_close_never_steals_newer_selection() {
    let snapshot = snapshot();
    let session = &snapshot.sessions[0];
    let mut selection = WindowSelection::new(InitialSelection::Recent);
    selection.accept_snapshot(&snapshot);
    selection.select_tab(&snapshot, session.id, session.tabs[1].id);
    let epoch = selection.epoch();
    assert!(selection.tab_closed(session, session.tabs[1].id, epoch));
    assert_eq!(selection.selected().tab_id, Some(session.tabs[2].id));
    let epoch = selection.epoch();
    let mut after_first_close = session.clone();
    after_first_close.tabs[1].state = TabState::Archived;
    assert!(selection.tab_closed(&after_first_close, session.tabs[2].id, epoch));
    assert_eq!(selection.selected().tab_id, Some(session.tabs[0].id));
    let epoch = selection.epoch();
    selection.select_session(&snapshot, snapshot.sessions[1].id);
    assert!(!selection.tab_closed(session, session.tabs[0].id, epoch));
    assert_eq!(
        selection.selected().session_id,
        Some(snapshot.sessions[1].id)
    );
}
#[test]
fn deletion_repairs_local_selection_archives_are_hidden_and_old_snapshots_are_ignored() {
    let mut snapshot = snapshot();
    let old = snapshot.clone();
    let mut selection = WindowSelection::new(InitialSelection::Recent);
    selection.accept_snapshot(&snapshot);
    snapshot.sessions[0].tabs[0].state = TabState::Archived;
    snapshot.revision += 1;
    selection.accept_snapshot(&snapshot);
    assert_eq!(
        selection.selected().tab_id,
        Some(snapshot.sessions[0].tabs[1].id)
    );
    snapshot.sessions.remove(0);
    snapshot.revision += 1;
    selection.accept_snapshot(&snapshot);
    assert_eq!(
        selection.selected().session_id,
        Some(snapshot.sessions[0].id)
    );
    assert!(!selection.accept_snapshot(&old));
    assert_eq!(
        selection.selected().session_id,
        Some(snapshot.sessions[0].id)
    );
}
#[test]
fn remembered_tabs_are_bounded_and_pruned_on_session_deletion() {
    let mut snapshot = snapshot();
    snapshot.sessions = (0..MAX_REMEMBERED_SESSIONS + 10)
        .map(|_| session())
        .collect();
    let mut selection = WindowSelection::new(InitialSelection::Empty);
    selection.accept_snapshot(&snapshot);
    for session in &snapshot.sessions {
        selection.select_tab(&snapshot, session.id, session.tabs[2].id);
    }
    assert_eq!(selection.remembered.len(), MAX_REMEMBERED_SESSIONS);
    snapshot.sessions.clear();
    snapshot.revision += 1;
    selection.accept_snapshot(&snapshot);
    assert!(selection.remembered.is_empty());
    assert_eq!(*selection.selected(), Selection::default());
}

#[derive(Default)]
struct AttachFake(std::sync::Mutex<Vec<bool>>);
impl control::WindowAttach for AttachFake {
    fn attach(
        &self,
        _: TabId,
        takeover: bool,
    ) -> crate::app_host::HostFuture<'_, shellfox_service::Attachment> {
        self.0.lock().unwrap().push(takeover);
        Box::pin(async {
            Err(shellfox_runtime::RuntimeError {
                code: shellfox_runtime::ErrorCode::ControlRequired,
                message: "fixture denied".into(),
            }
            .into())
        })
    }
}
#[tokio::test(flavor = "current_thread")]
async fn opening_session_new_window_does_not_takeover_observe_or_retry() {
    let fake = AttachFake::default();
    let tab = TabId::new();
    assert!(matches!(
        control::attach(&fake, tab, control::AttachIntent::Open).await,
        control::AttachOutcome::ControlRequired
    ));
    assert_eq!(*fake.0.lock().unwrap(), [false]);
    assert!(matches!(
        control::attach(&fake, tab, control::AttachIntent::TakeIt).await,
        control::AttachOutcome::ControlRequired
    ));
    assert_eq!(*fake.0.lock().unwrap(), [false, true]);
    assert!(!control::is_control_required(&anyhow::anyhow!(
        "ControlRequired"
    )));
    let typed = anyhow::Error::new(shellfox_runtime::RuntimeError {
        code: shellfox_runtime::ErrorCode::Unavailable,
        message: "ControlRequired".into(),
    });
    assert!(!control::is_control_required(&typed));
}
