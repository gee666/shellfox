use chrono::{SecondsFormat, Utc};
use rusqlite::Connection;
use shellfox_host::*;
use std::path::PathBuf;
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("tab-timestamps-{}", SessionId::new()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn db(&self) -> Connection {
        Connection::open(self.0.join(STORE_FILENAME)).unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn request() -> CreateSession {
    CreateSession {
        request_id: SessionId::new(),
        title: None,
        cwd: "/work/project".into(),
        profile_id: ProfileId::default(),
    }
}
fn add() -> AddTab {
    AddTab {
        title: None,
        cwd: None,
        profile_id: ProfileId::default(),
    }
}
#[test]
fn new_shell_titles_use_session_ordinals_and_allow_explicit_names() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let initial = host.create_session(request()).unwrap();
    let session = initial.sessions[0].id;
    let first = &initial.sessions[0].tabs[0];
    assert_eq!(first.title, "Shell 1");
    let next = host.add_tab(session, add()).unwrap();
    assert_eq!(next.sessions[0].tabs[1].title, "Shell 2");
    host.close_tab(first.id).unwrap();
    let next = host.add_tab(session, add()).unwrap();
    assert_eq!(next.sessions[0].tabs[1].title, "Shell 3");
    let named = host
        .add_tab(
            session,
            AddTab {
                title: Some("Logs".into()),
                ..add()
            },
        )
        .unwrap();
    assert_eq!(named.sessions[0].tabs[2].title, "Logs");
}

fn assert_captured(created: Option<&str>, before: i64, after: i64) {
    let actual = chrono::DateTime::parse_from_rfc3339(
        created.expect("new tab intent must capture its own timestamp"),
    )
    .unwrap()
    .timestamp_millis();
    assert!(
        (before..=after).contains(&actual),
        "{actual} not captured within {before}..={after}"
    );
}
#[test]
fn new_tab_intents_capture_real_time_not_the_historical_session_birth_and_persist() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let input = request();
    let before = Utc::now().timestamp_millis();
    let first = host.create_session(input.clone()).unwrap();
    let after = Utc::now().timestamp_millis();
    let session = first.sessions[0].id;
    let original = first.sessions[0].tabs[0].created_at.clone();
    assert_captured(original.as_deref(), before, after);
    assert_eq!(
        host.create_session(input).unwrap(),
        first,
        "retry must not recreate the intent timestamp"
    );
    drop(host);
    // An isolated fixture with an older session proves add_tab must capture its
    // own time. Equal timestamps under a frozen/low-resolution clock remain legal.
    let historical =
        (Utc::now() - chrono::Duration::days(1)).to_rfc3339_opts(SecondsFormat::Millis, true);
    scratch
        .db()
        .execute(
            "UPDATE sessions SET value=json_set(value,'$.createdAt',?1)",
            [&historical],
        )
        .unwrap();
    let host = Host::open(&scratch.0).unwrap();
    let before = Utc::now().timestamp_millis();
    let next = host.add_tab(session, add()).unwrap();
    let after = Utc::now().timestamp_millis();
    let tab = &next.sessions[0].tabs[1];
    assert_captured(tab.created_at.as_deref(), before, after);
    assert_ne!(tab.created_at.as_deref(), Some(historical.as_str()));
    assert_eq!(next.sessions[0].tabs[0].created_at, original);
    let second = tab.id;
    let first_id = next.sessions[0].tabs[0].id;
    let second_time = tab.created_at.clone();
    host.rename_tab(second, "renamed").unwrap();
    host.reorder_tabs(session, &[second, first_id]).unwrap();
    let saved = host.snapshot().unwrap();
    assert_eq!(saved.sessions[0].tabs[0].created_at, second_time);
    drop(host);
    assert_eq!(Host::open(&scratch.0).unwrap().snapshot().unwrap(), saved);
}
#[test]
fn older_missing_tab_birth_stays_unknown_through_mutations_and_runtime_generations() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let snapshot = host.create_session(request()).unwrap();
    let session = snapshot.sessions[0].id;
    let old = snapshot.sessions[0].tabs[0].id;
    drop(host);
    scratch
        .db()
        .execute_batch("UPDATE tabs SET value=json_remove(value,'$.createdAt')")
        .unwrap();
    let host = Host::open(&scratch.0).unwrap();
    assert_eq!(
        host.snapshot().unwrap().sessions[0].tabs[0].created_at,
        None
    );
    host.rename_tab(old, "old prototype tab").unwrap();
    let identity = TerminalIdentity {
        tab_id: old,
        generation: Generation::new(),
    };
    host.register_terminal(identity).unwrap();
    host.terminal_stopped(identity).unwrap();
    let next = host.add_tab(session, add()).unwrap();
    assert_eq!(next.sessions[0].tabs[0].created_at, None);
    assert!(next.sessions[0].tabs[1].created_at.is_some());
    let saved = host.snapshot().unwrap();
    drop(host);
    let reopened = Host::open(&scratch.0).unwrap();
    assert_eq!(reopened.snapshot().unwrap(), saved);
    let db = scratch.db();
    let old_json: String = db
        .query_row(
            "SELECT value FROM tabs WHERE id=?",
            [old.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert!(serde_json::from_str::<serde_json::Value>(&old_json)
        .unwrap()
        .get("createdAt")
        .is_none());
}
#[test]
fn corrupt_persisted_known_tab_time_is_rejected_not_defaulted() {
    for time in ["invalid", "2026-07-07T12:00:00+02:00"] {
        let scratch = Scratch::new();
        let host = Host::open(&scratch.0).unwrap();
        host.create_session(request()).unwrap();
        drop(host);
        scratch
            .db()
            .execute(
                "UPDATE tabs SET value=json_set(value,'$.createdAt',?)",
                [time],
            )
            .unwrap();
        assert_eq!(
            Host::open(&scratch.0).err().unwrap().code,
            ErrorCode::StorageFailed
        );
    }
}
