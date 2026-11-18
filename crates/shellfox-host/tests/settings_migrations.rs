use rusqlite::Connection;
use serde_json::json;
use shellfox_host::*;
use std::path::PathBuf;
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("settings-migrate-{}", SessionId::new()));
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
#[test]
fn migrates_old_isolated_schema_one_without_losing_configuration_or_revision() {
    let dir = Scratch::new();
    let db = dir.db();
    db.execute_batch("CREATE TABLE host_state(id INTEGER PRIMARY KEY CHECK(id=1),value TEXT NOT NULL);
        CREATE TABLE sessions(id TEXT PRIMARY KEY,value TEXT NOT NULL);
        CREATE TABLE tabs(id TEXT PRIMARY KEY,session_id TEXT NOT NULL REFERENCES sessions(id),ordinal INTEGER NOT NULL,value TEXT NOT NULL,UNIQUE(session_id,ordinal));
        CREATE TABLE requests(request_id TEXT PRIMARY KEY,session_id TEXT NOT NULL REFERENCES sessions(id));
        PRAGMA user_version=1; PRAGMA application_id=1397119058;").unwrap();
    let state = json!({"revision":8,"preferences":{"fontFamily":"Local font","fontSize":17,"accentColor":"#123456","backgroundColor":"#abcdef","defaultProfileId":"wsl"},"selection":{"sessionId":null,"tabId":null},"terminalEnv":[{"name":"OLD_USER_VAR","value":"keep-me"}],"lastChange":{"revision":8,"reason":"settings","selectSessionId":null}});
    db.execute("INSERT INTO host_state VALUES(1,?)", [state.to_string()])
        .unwrap();
    drop(db);
    let host = Host::open(&dir.0).unwrap();
    let settings = host.global_settings().unwrap();
    let snapshot = host.snapshot().unwrap();
    assert_eq!(snapshot.revision, 8);
    assert_eq!(snapshot.preferences.font_family, "Local font");
    assert_eq!(snapshot.preferences.font_size, 17.0);
    assert_eq!(settings.accent_color, "#123456");
    assert_eq!(settings.background_color, "#abcdef");
    assert_eq!(settings.terminal_profile_id, Some(Some("wsl".into())));
    assert_eq!(settings.terminal_env[0].value, "keep-me");
    assert_eq!(settings.shell_id, SettingsShell::Wsl);
    assert_eq!(host.changed_since(7).unwrap().unwrap().revision, 8);
    drop(host);
    let db = dir.db();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0))
            .unwrap(),
        5
    );
    drop(db);
    let host = Host::open(&dir.0).unwrap();
    assert_eq!(host.global_settings().unwrap(), settings);
    assert_eq!(host.snapshot().unwrap(), snapshot);
}
#[test]
fn missing_or_invalid_version_two_settings_fail_without_silent_repair() {
    for corruption in [
        "DROP TABLE global_settings",
        "DELETE FROM global_settings",
        "UPDATE global_settings SET value='{}'",
        "UPDATE global_settings SET value=json_set(value,'$.historyPageSize',0)",
    ] {
        let dir = Scratch::new();
        drop(Host::open(&dir.0).unwrap());
        let db = dir.db();
        db.execute_batch("DROP TABLE native_approvals; DROP TABLE native_approval_state; DROP TABLE reserved_publications; DROP TABLE session_creation_facts; PRAGMA user_version=2;")
            .unwrap();
        db.execute_batch(corruption).unwrap();
        drop(db);
        assert!(Host::open(&dir.0).is_err(), "accepted {corruption}");
        let db = dir.db();
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0))
                .unwrap(),
            2
        );
    }
}
#[test]
fn corrupt_schema_one_missing_global_preferences_is_not_defaulted_during_migration() {
    let dir = Scratch::new();
    drop(Host::open(&dir.0).unwrap());
    // Downgrade only the isolated fixture and remove globals to model corruption.
    // A valid schema1 required full Preferences and terminalEnv.
    let db = dir.db();
    db.execute_batch(
        "DROP TABLE native_approvals; DROP TABLE native_approval_state; DROP TABLE reserved_publications; DROP TABLE session_creation_facts; DROP TABLE global_settings; PRAGMA user_version=1;",
    )
    .unwrap();
    drop(db);
    assert_eq!(
        Host::open(&dir.0).err().unwrap().code,
        ErrorCode::StorageFailed
    );
    let db = dir.db();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0))
            .unwrap(),
        1
    );
    let tables: i32 = db
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='global_settings'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tables, 0);
}
#[test]
fn failed_migration_is_atomic_and_does_not_repair_foreign_identity() {
    let dir = Scratch::new();
    drop(Host::open(&dir.0).unwrap());
    let db = dir.db();
    db.execute_batch("DROP TABLE native_approvals; DROP TABLE native_approval_state; PRAGMA user_version=1; PRAGMA application_id=0;")
        .unwrap();
    drop(db);
    assert_eq!(
        Host::open(&dir.0).err().unwrap().code,
        ErrorCode::Unsupported
    );
    let db = dir.db();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i32>(0))
            .unwrap(),
        1
    );
}
