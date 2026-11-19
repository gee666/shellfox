use shellfox_host::*;
use std::{path::PathBuf, sync::Arc};
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("host-contract-{}", SessionId::new()));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn create(host: &Host) -> Snapshot {
    host.create_session(CreateSession {
        request_id: SessionId::new(),
        title: Some("work".into()),
        cwd: "/work/project".into(),
        profile_id: ProfileId::new("login-shell").unwrap(),
    })
    .unwrap()
}
fn add() -> AddTab {
    AddTab {
        title: None,
        cwd: None,
        profile_id: ProfileId::new("cmd").unwrap(),
    }
}
fn identity(tab_id: TabId) -> TerminalIdentity {
    TerminalIdentity {
        tab_id,
        generation: Generation::new(),
    }
}

#[test]
fn sessions_tabs_selection_preferences_survive_reopen() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let initial = create(&host);
    let id = initial.sessions[0].id;
    let first = initial.sessions[0].tabs[0].id;
    let next = host.add_tab(id, add()).unwrap();
    let second = next.sessions[0].tabs[1].id;
    host.rename_session(id, "renamed").unwrap();
    host.rename_tab(first, "editor").unwrap();
    host.set_session_cwd(id, "C:\\work").unwrap();
    host.set_tab_cwd(first, "/work/other").unwrap();
    host.set_session_pinned(id, true).unwrap();
    host.set_session_settled(id, true).unwrap();
    host.reorder_tabs(id, &[second, first]).unwrap();
    host.select_tab(first).unwrap();
    let expected = host
        .update_preferences(PreferencesPatch {
            font_family: Some("JetBrains Mono".into()),
            font_size: Some(16.0),
            accent_color: Some("#abcdef".into()),
            background_color: Some("#123456".into()),
            default_profile_id: Some(ProfileId::new("pwsh").unwrap()),
        })
        .unwrap();
    assert_eq!(expected.selection.tab_id, Some(first));
    assert_eq!(expected.sessions[0].tabs[0].id, second);
    assert_eq!(expected.sessions[0].tabs[1].ordinal, 1);
    assert_eq!(expected.sessions[0].tabs[0].cwd, "/work/project");
    assert!(expected.sessions[0].pinned_at.is_some());
    assert!(expected.sessions[0].settled_at.is_some());
    drop(host);
    let reopened = Host::open(&scratch.0).unwrap();
    assert_eq!(reopened.snapshot().unwrap(), expected);
}

#[test]
fn create_request_is_idempotent_and_events_are_revisioned() {
    let scratch = Scratch::new();
    let host = Arc::new(Host::open(&scratch.0).unwrap());
    let input = CreateSession {
        request_id: SessionId::new(),
        title: None,
        cwd: "/work".into(),
        profile_id: ProfileId::default(),
    };
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let host = host.clone();
            let input = input.clone();
            std::thread::spawn(move || host.create_session(input).unwrap())
        })
        .collect();
    for thread in threads {
        assert_eq!(thread.join().unwrap().revision, 1);
    }
    let first = host.snapshot().unwrap();
    assert_eq!(first.sessions.len(), 1);
    assert_eq!(first.sessions[0].tabs.len(), 1);
    let event = host.changed_since(0).unwrap().unwrap();
    assert_eq!(event.revision, 1);
    assert_eq!(event.reason, ChangeReason::Sessions);
    assert!(host.changed_since(1).unwrap().is_none());
    let id = first.sessions[0].id;
    let threads: Vec<_> = (0..16)
        .map(|_| {
            let host = host.clone();
            std::thread::spawn(move || host.add_tab(id, add()).unwrap())
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    let snapshot = host.snapshot().unwrap();
    assert_eq!(snapshot.revision, 17);
    assert_eq!(snapshot.sessions[0].tabs.len(), 17);
    for (ordinal, tab) in snapshot.sessions[0].tabs.iter().enumerate() {
        assert_eq!(tab.ordinal, ordinal as u32);
    }
}

#[test]
fn invalid_mutations_are_atomic_and_unknown_capabilities_are_not_exposed() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let initial = create(&host);
    let id = initial.sessions[0].id;
    let tab = initial.sessions[0].tabs[0].id;
    assert_eq!(
        host.rename_session(id, "\nsecret").unwrap_err().code,
        ErrorCode::Validation
    );
    assert!(host.set_session_cwd(id, "../relative").is_err());
    assert!(host
        .update_preferences(PreferencesPatch {
            font_size: Some(f32::NAN),
            ..Default::default()
        })
        .is_err());
    assert!(host
        .update_preferences(PreferencesPatch {
            background_color: Some("red".into()),
            ..Default::default()
        })
        .is_err());
    assert!(host.reorder_tabs(id, &[tab, tab]).is_err());
    assert!(host.reorder_tabs(id, &[]).is_err());
    assert!(host
        .request_json(r#"{"method":"remote.connect","payload":{}}"#)
        .into_result()
        .is_err());
    assert!(host
        .request_json(r#"{"method":"getSnapshot","payload":{"command":"credential"}}"#)
        .into_result()
        .is_err());
    assert_eq!(host.snapshot().unwrap(), initial);
}

#[test]
fn cleanup_requires_current_generation_and_keeps_metadata_on_failure() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let initial = create(&host);
    let id = initial.sessions[0].id;
    let tab = initial.sessions[0].tabs[0].id;
    let owner = identity(tab);
    host.register_terminal(owner).unwrap();
    assert_eq!(
        host.close_tab(tab).unwrap_err().code,
        ErrorCode::CleanupRequired
    );
    assert_eq!(
        host.delete_session(id).unwrap_err().code,
        ErrorCode::CleanupRequired
    );
    assert!(host.terminal_stopped(identity(tab)).is_err());
    assert!(host.set_tab_cwd(tab, "/other").is_err());
    let prepared = host.prepare_close_tab(tab).unwrap();
    assert_eq!(prepared.terminals, vec![owner]);
    assert!(host.register_terminal(identity(tab)).is_err());
    assert_eq!(host.snapshot().unwrap(), initial);
    // Cleanup failure means do not commit. Retain registration/token until retry.
    host.rename_session(id, "changed during cleanup").unwrap();
    assert_eq!(
        host.commit_close(prepared.token).unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(
        host.close_tab(tab).unwrap_err().code,
        ErrorCode::CleanupRequired
    );
    host.cancel_close(prepared.token).unwrap();
    let retry = host.prepare_close_tab(tab).unwrap();
    host.terminal_stopped(owner).unwrap();
    let final_state = host.commit_close(retry.token).unwrap();
    assert!(final_state.sessions[0].tabs.is_empty());
    assert_eq!(final_state.selection.session_id, Some(id));
    assert_eq!(final_state.selection.tab_id, None);
    assert!(host.commit_close(retry.token).is_err());
    host.delete_session(id).unwrap();
    assert!(host.snapshot().unwrap().sessions.is_empty());
}

#[test]
fn delayed_close_confirmation_cannot_reserve_a_replacement_generation() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let initial = create(&host);
    let tab = initial.sessions[0].tabs[0].id;
    let shown = identity(tab);
    assert_eq!(host.prepare_close_terminal(shown).unwrap_err().code, ErrorCode::Conflict);
    host.register_terminal(shown).unwrap();
    host.terminal_stopped(shown).unwrap();
    let replacement = identity(tab);
    host.register_terminal(replacement).unwrap();
    assert_eq!(host.prepare_close_terminal(shown).unwrap_err().code, ErrorCode::Conflict);
    assert_eq!(host.snapshot().unwrap(), initial);
    let prepared = host.prepare_close_terminal(replacement).unwrap();
    assert_eq!(prepared.terminals, [replacement]);
    host.terminal_stopped(replacement).unwrap();
    assert!(host.commit_close(prepared.token).unwrap().sessions[0].tabs.is_empty());
}

#[test]
fn deleting_session_cleans_all_children_and_selection() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let first = create(&host);
    let id = first.sessions[0].id;
    host.add_tab(id, add()).unwrap();
    let other = create(&host).selection.session_id.unwrap();
    host.select_session(id).unwrap();
    let snapshot = host.delete_session(id).unwrap();
    assert_eq!(snapshot.sessions.len(), 1);
    assert_eq!(snapshot.selection.session_id, Some(other));
    drop(host);
    let db = rusqlite::Connection::open(scratch.0.join(STORE_FILENAME)).unwrap();
    let orphan_count: i64 = db
        .query_row(
            "SELECT count(*) FROM tabs WHERE session_id NOT IN (SELECT id FROM sessions)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(orphan_count, 0);
    assert_eq!(
        db.query_row("SELECT count(*) FROM requests", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn explicit_env_overrides_global_without_inheriting_process_values() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let snapshot = create(&host);
    let id = snapshot.sessions[0].id;
    let tab = snapshot.sessions[0].tabs[0].id;
    assert!(host.terminal_launch(tab).unwrap().env.is_empty());
    host.set_terminal_env(vec![EnvVar {
        name: "A".into(),
        value: "global".into(),
    }])
    .unwrap();
    host.save_settings(
        PreferencesPatch {
            font_size: Some(15.0),
            ..Default::default()
        },
        None,
    )
    .unwrap();
    assert_eq!(host.terminal_launch(tab).unwrap().env[0].value, "global");
    host.set_session_env(
        id,
        vec![EnvVar {
            name: "A".into(),
            value: "local".into(),
        }],
    )
    .unwrap();
    assert_eq!(
        host.terminal_launch(tab).unwrap().env,
        vec![EnvVar {
            name: "A".into(),
            value: "local".into()
        }]
    );
    assert!(host
        .set_terminal_env(vec![EnvVar {
            name: "A".into(),
            value: "line\nsecret".into()
        }])
        .is_err());
    assert!(host
        .set_session_env(
            id,
            vec![
                EnvVar {
                    name: "A".into(),
                    value: "1".into()
                },
                EnvVar {
                    name: "A".into(),
                    value: "2".into()
                }
            ]
        )
        .is_err());
}

#[test]
fn isolated_identity_newer_version_and_concurrent_owner_are_refused() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    assert_eq!(
        Host::open(&scratch.0).err().unwrap().code,
        ErrorCode::Conflict
    );
    drop(host);
    let db = rusqlite::Connection::open(scratch.0.join(STORE_FILENAME)).unwrap();
    assert_eq!(
        db.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    let current: i64 = db.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
    db.pragma_update(None, "user_version", current + 1).unwrap();
    drop(db);
    assert_eq!(
        Host::open(&scratch.0).err().unwrap().code,
        ErrorCode::Unsupported
    );
    let db = rusqlite::Connection::open(scratch.0.join(STORE_FILENAME)).unwrap();
    db.pragma_update(None, "user_version", current).unwrap();
    db.pragma_update(None, "application_id", 0).unwrap();
    drop(db);
    assert_eq!(
        Host::open(&scratch.0).err().unwrap().code,
        ErrorCode::Unsupported
    );
}

#[test]
fn persisted_invalid_state_is_rejected_not_defaulted() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    create(&host);
    drop(host);
    let db = rusqlite::Connection::open(scratch.0.join(STORE_FILENAME)).unwrap();
    db.execute(
        "UPDATE sessions SET value=json_set(value,'$.cwd','relative')",
        [],
    )
    .unwrap();
    drop(db);
    assert!(Host::open(&scratch.0).is_err());
}
