use super::*;
use shellfox_host::{ProfileId, Selection, Session, Tab, TabState};
fn session() -> Session {
    let id = SessionId::new();
    Session {
        id,
        title: "owned".into(),
        cwd: if cfg!(windows) { "C:/owned" } else { "/owned" }.into(),
        created_at: "2026-11-16T00:00:00.000Z".into(),
        updated_at: "2026-11-16T00:00:00.000Z".into(),
        settled_at: None,
        pinned_at: None,
        env: vec![],
        tabs: vec![Tab {
            id: TabId::new(),
            session_id: id,
            title: "shell".into(),
            use_process_title: false,
            cwd: if cfg!(windows) { "C:/owned" } else { "/owned" }.into(),
            ordinal: 0,
            created_at: None,
            profile_id: ProfileId::new("login-shell").unwrap(),
            wsl_launch: None,
            state: TabState::Pending,
        }],
    }
}
fn reply() -> Snapshot {
    let sessions = vec![session(), session()];
    let selected = &sessions[0];
    Snapshot {
        revision: 1,
        selection: Selection {
            session_id: Some(selected.id),
            tab_id: Some(selected.tabs[0].id),
        },
        sessions,
        preferences: Default::default(),
    }
}
#[test]
fn delayed_history_activation_does_not_open_a_shell_after_a_new_selection() {
    use crate::window_manager::{InitialSelection, WindowSelection};

    let mut delayed = reply();
    delayed.sessions[0].tabs.clear();
    delayed.selection.tab_id = None;
    let first = delayed.sessions[0].id;
    let last = &delayed.sessions[1];
    let mut nav = WindowSelection::new(InitialSelection::Empty);
    assert!(nav.accept_snapshot(&delayed));
    assert!(nav.select_session(&delayed, first));
    let fence = |nav: &WindowSelection, source| ActivationFence { source, navigation: nav.epoch() };
    let pending = (first, fence(&nav, 1));
    assert!(activation_reopens_history(pending, nav.selected().session_id, fence(&nav, 1), &delayed));
    assert!(!activation_reopens_history(pending, nav.selected().session_id, fence(&nav, 3), &delayed));

    assert!(nav.select_session(&delayed, last.id));
    assert!(!activation_reopens_history(pending, nav.selected().session_id, fence(&nav, 1), &delayed));
    // Reading the authoritative catalog is allowed, but its recent selection is not focus.
    assert!(nav.accept_snapshot(&delayed));
    assert_eq!(nav.selected().session_id, Some(last.id));
    assert_eq!(nav.selected().tab_id, Some(last.tabs[0].id));

    assert!(nav.select_session(&delayed, first));
    assert!(!activation_reopens_history(pending, nav.selected().session_id, fence(&nav, 1), &delayed));
    assert!(activation_reopens_history((first, fence(&nav, 1)), nav.selected().session_id, fence(&nav, 1), &delayed));
}

#[test]
fn creation_uuid_is_not_session_identity_and_actual_receipt_selects_reply_target() {
    let reply = reply();
    let request_id = SessionId::new();
    let actual = reply.sessions[0].id;
    assert_ne!(request_id, actual);
    assert_eq!(
        focus_target(
            ReplyIntent::Create,
            Some(CreationRequestReceipt::SessionCreated { session_id: actual }),
            &reply,
            true
        ),
        Some(ReplyFocus {
            session: actual,
            tab: Some(reply.sessions[0].tabs[0].id)
        })
    );
}
#[test]
fn idempotent_reply_selection_cannot_retarget_original_creation_receipt() {
    let reply = reply();
    let original = reply.sessions[1].id;
    assert_eq!(
        focus_target(
            ReplyIntent::Create,
            Some(CreationRequestReceipt::SessionCreated {
                session_id: original
            }),
            &reply,
            true
        ),
        Some(ReplyFocus {
            session: original,
            tab: None
        })
    );
}
#[test]
fn unavailable_missing_deleted_and_stale_epoch_never_guess_selected_or_newest_session() {
    let reply = reply();
    let intent = ReplyIntent::Create;
    for receipt in [
        None,
        Some(CreationRequestReceipt::ReceiptUnavailable),
        Some(CreationRequestReceipt::SessionCreated {
            session_id: SessionId::new(),
        }),
    ] {
        assert_eq!(focus_target(intent, receipt, &reply, true), None);
    }
    assert_eq!(
        focus_target(
            intent,
            Some(CreationRequestReceipt::SessionCreated {
                session_id: reply.sessions[0].id
            }),
            &reply,
            false
        ),
        None
    );
}
#[test]
fn add_tab_uses_requested_session_and_never_another_sessions_tab() {
    let reply = reply();
    let actual = reply.sessions[0].id;
    assert_eq!(
        focus_target(
            ReplyIntent::AddTab { session_id: actual },
            None,
            &reply,
            true
        ),
        Some(ReplyFocus {
            session: actual,
            tab: Some(reply.sessions[0].tabs[0].id)
        })
    );
    assert_eq!(
        focus_target(
            ReplyIntent::AddTab {
                session_id: reply.sessions[1].id
            },
            None,
            &reply,
            true
        ),
        Some(ReplyFocus {
            session: reply.sessions[1].id,
            tab: None
        })
    );
    assert_eq!(focus_target(ReplyIntent::None, None, &reply, true), None);
}
