use rusqlite::Connection;
use shellfox_host::*;
use std::{path::PathBuf, sync::Arc};
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("creation-facts-{}", SessionId::new()));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
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
fn input() -> CreateSession {
    CreateSession {
        request_id: SessionId::new(),
        title: None,
        cwd: "/work".into(),
        profile_id: ProfileId::new("cmd").unwrap(),
    }
}
fn facts() -> SessionCreationFacts {
    SessionCreationFacts::new(SettingsAdapter::EmbeddedPty, SettingsShell::LoginShell)
}
#[test]
fn metadata_creation_is_unknown_but_trusted_creation_persists_facts_without_snapshot_changes() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let unknown = host
        .create_session(input())
        .unwrap()
        .selection
        .session_id
        .unwrap();
    assert!(host.session_creation_facts().unwrap().is_empty());
    let known = host
        .create_session_with_facts(input(), facts())
        .unwrap()
        .selection
        .session_id
        .unwrap();
    let map = host.session_creation_facts().unwrap();
    assert!(!map.contains_key(&unknown));
    assert_eq!(map.get(&known), Some(&facts()));
    let snapshot = host.snapshot().unwrap();
    let encoded = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(encoded.as_object().unwrap().len(), 4);
    assert!(encoded.get("creationFacts").is_none());
    drop(host);
    let host = Host::open(&scratch.0).unwrap();
    assert_eq!(host.session_creation_facts().unwrap(), map);
    assert_eq!(host.snapshot().unwrap(), snapshot);
}
#[test]
fn retries_never_replace_known_facts_or_backfill_unknown_original_intent() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let request = input();
    let first = host
        .create_session_with_facts(request.clone(), facts())
        .unwrap();
    let other = SessionCreationFacts::new(SettingsAdapter::GnomeTerminal, SettingsShell::Bash);
    assert_eq!(
        host.create_session_with_facts(request, other.clone())
            .unwrap(),
        first
    );
    assert_eq!(
        host.session_creation_facts().unwrap().values().next(),
        Some(&facts())
    );
    let request = input();
    let metadata = host.create_session(request.clone()).unwrap();
    let id = metadata.selection.session_id.unwrap();
    assert_eq!(
        host.create_session_with_facts(request, other).unwrap(),
        metadata
    );
    assert!(!host.session_creation_facts().unwrap().contains_key(&id));
}
#[test]
fn facts_survive_later_profile_settings_activity_and_tab_mutations_then_delete_with_parent() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let snapshot = host.create_session_with_facts(input(), facts()).unwrap();
    let id = snapshot.selection.session_id.unwrap();
    host.add_tab(
        id,
        AddTab {
            title: None,
            cwd: None,
            profile_id: ProfileId::new("pwsh").unwrap(),
        },
    )
    .unwrap();
    host.rename_session(id, "renamed").unwrap();
    host.set_session_cwd(id, "/other").unwrap();
    host.set_session_settled(id, true).unwrap();
    host.set_session_settled(id, false).unwrap();
    host.save_global_settings(SettingsPatch {
        accent_color: Some("#123456".into()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        host.session_creation_facts().unwrap().get(&id),
        Some(&facts())
    );
    let second = host
        .create_session_with_facts(input(), facts())
        .unwrap()
        .selection
        .session_id
        .unwrap();
    host.delete_session(id).unwrap();
    let map = host.session_creation_facts().unwrap();
    assert!(!map.contains_key(&id));
    assert_eq!(map.get(&second), Some(&facts()));
    let prepared = host.prepare_delete_session(second).unwrap();
    host.commit_close(prepared.token).unwrap();
    assert!(host.session_creation_facts().unwrap().is_empty());
}
#[test]
fn intent_facts_request_and_revision_are_one_atomic_transaction() {
    for (table, event) in [
        ("session_creation_facts", "INSERT"),
        ("host_state", "UPDATE"),
    ] {
        let scratch = Scratch::new();
        let host = Host::open(&scratch.0).unwrap();
        let before = host.snapshot().unwrap();
        let request = input();
        scratch.db().execute_batch(&format!("CREATE TRIGGER fail_creation BEFORE {event} ON {table} BEGIN SELECT RAISE(ABORT,'injected failure'); END;")).unwrap();
        assert_eq!(
            host.create_session_with_facts(request.clone(), facts())
                .unwrap_err()
                .code,
            ErrorCode::StorageFailed
        );
        assert_eq!(host.snapshot().unwrap(), before);
        assert!(host.session_creation_facts().unwrap().is_empty());
        assert!(host.changed_since(0).unwrap().is_none());
        scratch
            .db()
            .execute_batch("DROP TRIGGER fail_creation")
            .unwrap();
        let saved = host
            .create_session_with_facts(request.clone(), facts())
            .unwrap();
        assert_eq!(saved.sessions.len(), 1);
        assert_eq!(saved.revision, 1);
        assert_eq!(
            host.create_session_with_facts(request, facts()).unwrap(),
            saved
        );
    }
}
struct Lost;
impl SettingsRuntime for Lost {
    fn normalize(&self, _: &Settings, candidate: Settings) -> Result<Settings> {
        Ok(candidate)
    }
    fn check_commit(&self) -> Result<()> {
        Err(Error::new(ErrorCode::Internal, "Native ownership lost."))
    }
}
#[test]
fn final_native_guard_rolls_back_creation_and_invalid_fact_version_is_not_defaulted() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let mut invalid = facts();
    invalid.version = 2;
    assert_eq!(
        host.create_session_with_facts(input(), invalid)
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
    host.install_settings_runtime(Arc::new(Lost)).unwrap();
    assert_eq!(
        host.create_session_with_facts(input(), facts())
            .unwrap_err()
            .code,
        ErrorCode::Internal
    );
    assert!(host.session_creation_facts().unwrap().is_empty());
    assert!(host.snapshot().unwrap().sessions.is_empty());
    assert_eq!(host.snapshot().unwrap().revision, 0);
}
#[test]
fn present_corrupt_missing_or_orphan_facts_fail_closed_without_repair() {
    for corruption in [
        "UPDATE session_creation_facts SET value='{}'",
        "UPDATE session_creation_facts SET value=json_set(value,'$.version',2)",
        "UPDATE session_creation_facts SET value=json_set(value,'$.shellId','cmd')",
        "DROP TABLE session_creation_facts",
        "PRAGMA foreign_keys=OFF;DELETE FROM requests;DELETE FROM tabs;DELETE FROM sessions;",
    ] {
        let scratch = Scratch::new();
        let host = Host::open(&scratch.0).unwrap();
        host.create_session_with_facts(input(), facts()).unwrap();
        drop(host);
        scratch.db().execute_batch(corruption).unwrap();
        assert_eq!(
            Host::open(&scratch.0).err().unwrap().code,
            ErrorCode::StorageFailed,
            "{corruption}"
        );
    }
}
