use rusqlite::Connection;
use serde_json::json;
use shellfox_host::*;
use std::path::PathBuf;
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("facts-migrations-{}", SessionId::new()));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn db(&self) -> Connection {
        Connection::open(self.0.join(STORE_FILENAME)).unwrap()
    }
    fn source(&self) -> PathBuf {
        self.0.join("legacy-copy.sqlite3")
    }
    fn legacy(&self, version: i32) -> Connection {
        let db = Connection::open(self.source()).unwrap();
        db.execute_batch(include_str!("fixtures/legacy-v1.sql"))
            .unwrap();
        if version >= 2 {
            db.execute_batch("CREATE TABLE terminal_metadata(tabId TEXT PRIMARY KEY REFERENCES tabs(id),value TEXT NOT NULL)").unwrap();
        }
        if version >= 3 {
            db.execute_batch("ALTER TABLE sessions ADD COLUMN env TEXT NOT NULL DEFAULT '[]'")
                .unwrap();
        }
        if version >= 4 {
            db.execute_batch("ALTER TABLE sessions ADD COLUMN pinnedAt TEXT")
                .unwrap();
        }
        db.pragma_update(None, "user_version", version).unwrap();
        db
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn old_isolated_v1_and_v2_rows_remain_unknown_without_settings_or_first_tab_backfill() {
    for version in [1, 2] {
        let scratch = Scratch::new();
        let host = Host::open(&scratch.0).unwrap();
        let before = host
            .create_session(CreateSession {
                request_id: SessionId::new(),
                title: None,
                cwd: "/work".into(),
                profile_id: ProfileId::new("cmd").unwrap(),
            })
            .unwrap();
        let settings = host.global_settings().unwrap();
        let changed = host.changed_since(0).unwrap();
        drop(host);
        let db = scratch.db();
        db.execute_batch("DROP TABLE native_approvals; DROP TABLE native_approval_state; DROP TABLE reserved_publications; DROP TABLE session_creation_facts")
            .unwrap();
        if version == 1 {
            let old = json!({"revision":before.revision,"preferences":before.preferences,"selection":before.selection,"terminalEnv":settings.terminal_env,"lastChange":changed});
            db.execute("UPDATE host_state SET value=?", [old.to_string()])
                .unwrap();
            db.execute_batch("DROP TABLE global_settings").unwrap();
        }
        db.pragma_update(None, "user_version", version).unwrap();
        drop(db);
        let host = Host::open(&scratch.0).unwrap();
        assert_eq!(host.snapshot().unwrap(), before);
        assert!(host.session_creation_facts().unwrap().is_empty());
        assert_eq!(
            scratch
                .db()
                .pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0))
                .unwrap(),
            5
        );
    }
}
#[test]
fn all_legacy_copy_versions_preserve_exact_validated_creation_columns_not_current_settings() {
    for version in 1..=5 {
        let scratch = Scratch::new();
        let db = scratch.legacy(version);
        db.execute_batch("UPDATE sessions SET adapterId='gnome-terminal',shellId='bash';UPDATE settings SET value=json_set(value,'$.adapterId','embedded-pty','$.shellId','pwsh');").unwrap();
        drop(db);
        let bytes = std::fs::read(scratch.source()).unwrap();
        let host = Host::open(scratch.0.join("destination")).unwrap();
        let imported = host.import_legacy(scratch.source()).unwrap();
        let id = imported.sessions[0].id;
        assert!(imported.sessions[0].tabs.is_empty());
        assert_eq!(
            host.session_creation_facts().unwrap().get(&id),
            Some(&SessionCreationFacts::new(
                SettingsAdapter::GnomeTerminal,
                SettingsShell::Bash
            ))
        );
        assert_eq!(
            host.global_settings().unwrap().adapter_id,
            SettingsAdapter::EmbeddedPty
        );
        assert_eq!(
            host.global_settings().unwrap().shell_id,
            SettingsShell::Pwsh
        );
        assert_eq!(std::fs::read(scratch.source()).unwrap(), bytes);
        drop(host);
        let host = Host::open(scratch.0.join("destination")).unwrap();
        assert_eq!(
            host.session_creation_facts()
                .unwrap()
                .get(&id)
                .unwrap()
                .shell_id,
            SettingsShell::Bash
        );
    }
}
#[test]
fn invalid_present_legacy_creation_columns_reject_the_entire_copy_import() {
    for mutation in [
        "UPDATE sessions SET adapterId='unknown-backend'",
        "UPDATE sessions SET shellId='cmd'",
        "UPDATE sessions SET adapterId=char(27)||'private-value'",
    ] {
        let scratch = Scratch::new();
        let db = scratch.legacy(5);
        db.execute_batch(mutation).unwrap();
        drop(db);
        let before = std::fs::read(scratch.source()).unwrap();
        let host = Host::open(scratch.0.join("destination")).unwrap();
        assert_eq!(
            host.import_legacy(scratch.source()).unwrap_err().code,
            ErrorCode::Validation
        );
        assert!(host.session_creation_facts().unwrap().is_empty());
        assert!(host.snapshot().unwrap().sessions.is_empty());
        assert_eq!(host.snapshot().unwrap().revision, 0);
        assert_eq!(std::fs::read(scratch.source()).unwrap(), before);
    }
}
#[test]
fn legacy_facts_commit_failure_rolls_back_settings_sessions_and_revision_together() {
    let scratch = Scratch::new();
    drop(scratch.legacy(5));
    let destination = scratch.0.join("destination");
    let host = Host::open(&destination).unwrap();
    let before = host.global_settings().unwrap();
    Connection::open(destination.join(STORE_FILENAME)).unwrap().execute_batch("CREATE TRIGGER fail_facts BEFORE INSERT ON session_creation_facts BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    assert_eq!(
        host.import_legacy(scratch.source()).unwrap_err().code,
        ErrorCode::StorageFailed
    );
    assert!(host.session_creation_facts().unwrap().is_empty());
    assert!(host.snapshot().unwrap().sessions.is_empty());
    assert_eq!(host.snapshot().unwrap().revision, 0);
    assert_eq!(host.global_settings().unwrap(), before);
}
