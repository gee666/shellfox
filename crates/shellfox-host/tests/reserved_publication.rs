use shellfox_host::*;
#[path = "support/reserved_publication.rs"]
mod support;
use support::*;

#[test]
fn exact_ids_publish_together_without_selection_or_runtime_ownership() {
    for existing in [false, true] {
        let scratch = Scratch::new();
        let host = Host::open(&scratch.0).unwrap();
        if existing {
            host.create_session(ordinary(SessionId::new())).unwrap();
        }
        let before = host.snapshot().unwrap();
        let request = input();
        assert_eq!(
            host.reserved_publication_receipt(request.request_id)
                .unwrap(),
            ReservedPublicationReceipt::ReceiptUnavailable
        );
        let saved = host
            .publish_reserved_session(request.clone(), Some(facts()))
            .unwrap();
        assert_eq!(saved.selection, before.selection);
        assert_eq!(saved.revision, before.revision + 1);
        let session = saved
            .sessions
            .iter()
            .find(|s| s.id == request.session_id)
            .unwrap();
        assert_eq!(session.title, "external run");
        assert_eq!(session.cwd, request.cwd);
        assert_eq!(session.tabs.len(), 1);
        let tab = &session.tabs[0];
        assert_eq!(tab.id, request.tab_id);
        assert_eq!(tab.session_id, request.session_id);
        assert_eq!(tab.profile_id, request.profile_id);
        assert_eq!(tab.state, TabState::Pending);
        let event = host.changed_since(before.revision).unwrap().unwrap();
        assert_eq!(event.revision, saved.revision);
        assert_eq!(event.reason, ChangeReason::Sessions);
        assert_eq!(event.select_session_id, None);
        assert_eq!(
            host.session_creation_facts()
                .unwrap()
                .get(&request.session_id),
            Some(&facts())
        );
        assert_eq!(
            host.creation_request_receipt(request.request_id).unwrap(),
            CreationRequestReceipt::ReceiptUnavailable
        );
        let prepared = host.prepare_delete_session(request.session_id).unwrap();
        assert!(prepared.terminals.is_empty());
        host.cancel_close(prepared.token).unwrap();
        let columns: Vec<String> = scratch
            .db()
            .prepare("PRAGMA table_info(reserved_publications)")
            .unwrap()
            .query_map([], |r| r.get(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(columns, ["version", "request_id", "session_id", "tab_id"]);
        let value = serde_json::to_value(&saved).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 4);
        assert!(!value.to_string().contains("generation"));
        drop(host);
        let host = Host::open(&scratch.0).unwrap();
        assert_eq!(host.snapshot().unwrap(), saved);
        assert_eq!(
            host.reserved_publication_receipt(request.request_id)
                .unwrap(),
            ReservedPublicationReceipt::Published {
                session_id: request.session_id,
                tab_id: request.tab_id
            }
        );
        assert_eq!(
            host.publish_reserved_session(request, Some(facts()))
                .unwrap(),
            saved
        );
    }
}

#[test]
fn retries_preserve_original_attributes_facts_and_receipt_not_first_or_selected_tab() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let request = input();
    let first = host
        .publish_reserved_session(request.clone(), Some(facts()))
        .unwrap();
    let mut retry = request.clone();
    retry.title = Some("replacement title".into());
    retry.cwd = "/elsewhere".into();
    retry.profile_id = ProfileId::new("pwsh").unwrap();
    let other_facts =
        SessionCreationFacts::new(SettingsAdapter::GnomeTerminal, SettingsShell::Bash);
    assert_eq!(
        host.publish_reserved_session(retry.clone(), Some(other_facts.clone()))
            .unwrap(),
        first
    );
    host.add_tab(
        request.session_id,
        AddTab {
            title: None,
            cwd: None,
            profile_id: ProfileId::default(),
        },
    )
    .unwrap();
    let selected = host.snapshot().unwrap().selection.tab_id.unwrap();
    assert_ne!(selected, request.tab_id);
    host.reorder_tabs(request.session_id, &[selected, request.tab_id])
        .unwrap();
    host.rename_session(request.session_id, "edited by user")
        .unwrap();
    let before = host.snapshot().unwrap();
    drop(host);
    let host = Host::open(&scratch.0).unwrap();
    assert_eq!(
        host.reserved_publication_receipt(request.request_id)
            .unwrap(),
        ReservedPublicationReceipt::Published {
            session_id: request.session_id,
            tab_id: request.tab_id
        }
    );
    assert_eq!(
        host.publish_reserved_session(retry, Some(other_facts))
            .unwrap(),
        before
    );
    assert_eq!(
        host.session_creation_facts()
            .unwrap()
            .get(&request.session_id),
        Some(&facts())
    );
    assert!(host.changed_since(before.revision).unwrap().is_none());
}

#[test]
fn request_session_and_tab_collisions_reject_without_mutation_in_both_creation_paths() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let request = input();
    let saved = host
        .publish_reserved_session(request.clone(), Some(facts()))
        .unwrap();
    let mut mismatched_session = request.clone();
    mismatched_session.session_id = SessionId::new();
    let mut mismatched_tab = request.clone();
    mismatched_tab.tab_id = TabId::new();
    let mut reused_session = input();
    reused_session.session_id = request.session_id;
    let mut reused_tab = input();
    reused_tab.tab_id = request.tab_id;
    for collision in [
        mismatched_session,
        mismatched_tab,
        reused_session,
        reused_tab,
    ] {
        assert_eq!(
            host.publish_reserved_session(collision, Some(facts()))
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(host.snapshot().unwrap(), saved);
    }
    assert_eq!(
        host.create_session(ordinary(request.request_id))
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        host.create_session_with_facts(ordinary(request.request_id), facts())
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    let ordinary_id = SessionId::new();
    let before = host.create_session(ordinary(ordinary_id)).unwrap();
    let session = before
        .sessions
        .iter()
        .find(|s| s.id != request.session_id)
        .unwrap();
    let mut reused_request = input();
    reused_request.request_id = ordinary_id;
    let mut reused_session = input();
    reused_session.session_id = session.id;
    let mut reused_tab = input();
    reused_tab.tab_id = session.tabs[0].id;
    for collision in [reused_request, reused_session, reused_tab] {
        assert_eq!(
            host.publish_reserved_session(collision, Some(facts()))
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(host.snapshot().unwrap(), before);
    }
}

#[test]
fn deleting_original_tab_or_session_keeps_tombstone_across_saves_and_reopen() {
    for delete in ["tab", "session", "prepared-tab", "prepared-session"] {
        let scratch = Scratch::new();
        let host = Host::open(&scratch.0).unwrap();
        let request = input();
        host.publish_reserved_session(request.clone(), Some(facts()))
            .unwrap();
        host.add_tab(
            request.session_id,
            AddTab {
                title: None,
                cwd: None,
                profile_id: ProfileId::default(),
            },
        )
        .unwrap();
        match delete {
            "tab" => {
                host.close_tab(request.tab_id).unwrap();
            }
            "session" => {
                host.delete_session(request.session_id).unwrap();
            }
            _ => {
                let prepared = if delete == "prepared-tab" {
                    host.prepare_close_tab(request.tab_id).unwrap()
                } else {
                    host.prepare_delete_session(request.session_id).unwrap()
                };
                assert!(prepared.terminals.is_empty());
                host.commit_close(prepared.token).unwrap();
            }
        }
        host.create_session(ordinary(SessionId::new())).unwrap();
        let before = host.snapshot().unwrap();
        drop(host);
        let host = Host::open(&scratch.0).unwrap();
        assert_eq!(
            host.reserved_publication_receipt(request.request_id)
                .unwrap(),
            ReservedPublicationReceipt::TargetDeleted {
                session_id: request.session_id,
                tab_id: request.tab_id
            }
        );
        assert_eq!(
            host.publish_reserved_session(request.clone(), Some(facts()))
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let mut same_ids = request.clone();
        same_ids.request_id = SessionId::new();
        assert_eq!(
            host.publish_reserved_session(same_ids, Some(facts()))
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        let mut wrong_ids = request.clone();
        wrong_ids.tab_id = TabId::new();
        assert_eq!(
            host.publish_reserved_session(wrong_ids, Some(facts()))
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(
            host.create_session(ordinary(request.request_id))
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(host.snapshot().unwrap(), before);
    }
}

#[test]
fn invalid_intent_or_facts_and_wire_requests_cannot_publish() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let before = host.snapshot().unwrap();
    let mut invalid_cwd = input();
    invalid_cwd.cwd = "relative".into();
    let mut invalid_title = input();
    invalid_title.title = Some("\0bad".into());
    for invalid in [invalid_cwd, invalid_title] {
        assert_eq!(
            host.publish_reserved_session(invalid, Some(facts()))
                .unwrap_err()
                .code,
            ErrorCode::Validation
        );
    }
    let mut invalid = facts();
    invalid.version = 2;
    assert_eq!(
        host.publish_reserved_session(input(), Some(invalid))
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    for method in ["publishReservedSession", "reservedPublicationReceipt"] {
        let wire = serde_json::json!({"method":method,"payload":{}});
        assert!(host.request_json(&wire.to_string()).into_result().is_err());
    }
    assert_eq!(host.snapshot().unwrap(), before);
}
