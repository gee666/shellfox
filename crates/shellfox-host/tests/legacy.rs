use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::Connection;
use shellfox_core::{ErrorCode, Preferences, Snapshot};
use shellfox_host::Host;

const SESSION: &str = "f919fb1a-fb03-4a93-8b9b-1cde465d5870";
const PIN: &str = "2025-02-04T04:05:06.000Z";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new(version: u32) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let fixture = Self(root.join(format!(
            "legacy-tests-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )));
        fs::create_dir_all(&fixture.0).unwrap();
        let db = Connection::open(fixture.source()).unwrap();
        db.execute_batch(include_str!("fixtures/legacy-v1.sql"))
            .unwrap();
        if version >= 2 {
            db.execute_batch("CREATE TABLE terminal_metadata (tabId TEXT PRIMARY KEY REFERENCES tabs(id), value TEXT NOT NULL);
                INSERT INTO terminal_metadata SELECT id, '{\"kind\":\"embedded\",\"profileId\":\"bash\",\"exitCode\":null}' FROM tabs;").unwrap();
        }
        if version >= 3 {
            db.execute_batch("ALTER TABLE sessions ADD COLUMN env TEXT NOT NULL DEFAULT '[]';
                UPDATE sessions SET env='[{\"name\":\"SECRET\",\"value\":\"source-only-secret\"}]';").unwrap();
        }
        if version >= 4 {
            db.execute_batch("ALTER TABLE sessions ADD COLUMN pinnedAt TEXT")
                .unwrap();
            db.execute("UPDATE sessions SET pinnedAt=?1", [PIN])
                .unwrap();
        }
        db.pragma_update(None, "user_version", version).unwrap();
        fixture
    }
    fn source(&self) -> PathBuf {
        self.0.join("source.sqlite3")
    }
    fn host(&self) -> Host {
        Host::open(self.0.join("host")).unwrap()
    }
    fn edit(&self, sql: &str) {
        let db = Connection::open(self.source()).unwrap();
        db.pragma_update(None, "foreign_keys", false).unwrap();
        db.execute_batch(sql).unwrap();
    }
    fn settings(&self, value: impl ToString) {
        Connection::open(self.source())
            .unwrap()
            .execute("UPDATE settings SET value=?1", [value.to_string()])
            .unwrap();
    }
    fn import(&self) -> Snapshot {
        self.host().import_legacy(self.source()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

// Compare file bytes as well as schema and every stored cell, including excluded data.
fn source_state(path: &Path) -> (Vec<u8>, Vec<String>) {
    let bytes = fs::read(path).unwrap();
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut schema = db
        .prepare("SELECT name || ':' || coalesce(sql,'') FROM sqlite_schema ORDER BY name")
        .unwrap();
    let mut state: Vec<String> = schema
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for table in [
        "sessions",
        "tabs",
        "operations",
        "settings",
        "preferences",
        "terminal_metadata",
    ] {
        let exists: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name=?1)",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        if !exists {
            continue;
        }
        let mut statement = db
            .prepare(&format!("SELECT * FROM {table} ORDER BY 1"))
            .unwrap();
        let columns = statement.column_count();
        let rows = statement
            .query_map([], |row| {
                Ok((0..columns)
                    .map(|column| format!("{:?}", row.get_ref(column).unwrap()))
                    .collect::<Vec<_>>()
                    .join("|"))
            })
            .unwrap();
        state.extend(rows.map(Result::unwrap));
    }
    state.push(
        db.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap()
            .to_string(),
    );
    (bytes, state)
}

#[test]
fn imports_all_five_versions_without_changing_source() {
    for version in 1..=5 {
        let fixture = Fixture::new(version);
        let before = source_state(&fixture.source());
        let host = fixture.host();
        let imported = host.import_legacy(fixture.source()).unwrap();
        assert_eq!(imported.sessions.len(), 1, "version {version}");
        let session = &imported.sessions[0];
        assert_eq!(session.id.to_string(), SESSION);
        assert_eq!(session.title, "Legacy project");
        assert_eq!(session.cwd, "/home/user/project");
        assert_eq!(session.created_at, "2025-01-01T01:02:03.000Z");
        assert_eq!(session.updated_at, "2025-02-02T02:03:04.000Z");
        assert_eq!(
            session.settled_at.as_deref(),
            Some("2025-02-03T03:04:05.000Z")
        );
        assert_eq!(session.pinned_at.as_deref(), (version >= 4).then_some(PIN));
        assert!(session.tabs.is_empty(), "saved shells must not be restored");
        assert!(
            session.env.is_empty(),
            "source environment must not be imported"
        );
        assert_eq!(imported.preferences.accent_color, "#aBcD09");
        assert_eq!(imported.preferences.background_color, "#123456");
        assert_eq!(
            imported.preferences.default_profile_id.as_str(),
            "login-shell"
        );
        assert_eq!(
            imported.preferences.font_family,
            Preferences::default().font_family
        );
        assert_eq!(
            imported.preferences.font_size,
            Preferences::default().font_size
        );
        let public = serde_json::to_string(&imported).unwrap();
        for excluded in [
            "source-only-secret",
            "987654321",
            "Saved live shell",
            "registered",
        ] {
            assert!(!public.contains(excluded));
        }
        assert_eq!(source_state(&fixture.source()), before);
        drop(host);
        assert_eq!(fixture.host().snapshot().unwrap(), imported);
    }
}

fn rejected(fixture: &Fixture, expected: ErrorCode) {
    let before = fs::read(fixture.source()).unwrap();
    let host = fixture.host();
    let empty = host.snapshot().unwrap();
    let error = host
        .import_legacy(fixture.source())
        .expect_err("import must fail");
    assert_eq!(error.code, expected, "{error}");
    assert!(!error.message.contains("source-only-secret"));
    assert!(!error.message.contains("source.sqlite3"));
    assert_eq!(
        host.snapshot().unwrap(),
        empty,
        "a failed import must be atomic"
    );
    assert_eq!(fs::read(fixture.source()).unwrap(), before);
}

#[test]
fn unsupported_and_unversioned_sources_are_not_migrated() {
    for version in [0, 6, 100] {
        let fixture = Fixture::new(5);
        fixture.edit(&format!("PRAGMA user_version={version}"));
        rejected(&fixture, ErrorCode::Unsupported);
    }
}

#[test]
fn invalid_imported_session_fields_reject_the_whole_import() {
    for sql in [
        "UPDATE sessions SET id='not-a-uuid'",
        "UPDATE sessions SET title='   '",
        "UPDATE sessions SET title=char(27)||'source-only-secret'",
        "UPDATE sessions SET cwd='relative/project'",
        "UPDATE sessions SET cwd='https://example.test/source-only-secret'",
        "UPDATE sessions SET createdAt='not-a-time'",
        "UPDATE sessions SET updatedAt='2025-01-01T00:00:00+02:00'",
        "UPDATE sessions SET settledAt='invalid'",
        "UPDATE sessions SET pinnedAt='invalid'",
    ] {
        let fixture = Fixture::new(5);
        fixture.edit(sql);
        rejected(&fixture, ErrorCode::Validation);
    }
}

#[test]
fn malformed_settings_are_not_replaced_with_defaults() {
    for value in [
        "not json",
        "null",
        "[]",
        "{}",
        r##"{"version":2,"accentColor":"#123456","shellId":"bash"}"##,
        r##"{"version":1,"accentColor":"red","shellId":"bash"}"##,
        r##"{"version":1,"accentColor":"#123456","backgroundColor":null,"shellId":"bash"}"##,
        r##"{"version":1,"accentColor":"#123456","shellId":"bad-shell"}"##,
        r##"{"version":1,"accentColor":"#123456","shellId":"bash","terminalProfileId":42}"##,
    ] {
        let fixture = Fixture::new(5);
        fixture.settings(value);
        rejected(&fixture, ErrorCode::Validation);
    }
    let fixture = Fixture::new(5);
    fixture.edit("DELETE FROM settings");
    rejected(&fixture, ErrorCode::Validation);
}

#[test]
fn default_profile_preserves_opaque_metadata_without_granting_execution() {
    for (legacy, portable) in [
        ("bash", "bash"),
        ("pwsh", "pwsh"),
        ("windows-powershell", "windows-powershell"),
        ("wsl", "wsl"),
        ("cmd", "cmd"),
        ("wsl:Ubuntu", "wsl:Ubuntu"),
        ("shell:/bin/zsh", "shell:/bin/zsh"),
        ("wsl:Opaque distro (dev)", "wsl:Opaque distro (dev)"),
    ] {
        let fixture = Fixture::new(5);
        let settings = serde_json::json!({"version":1,"accentColor":"#123456","adapterId":"embedded-pty","shellId":"bash","shellExecutable":null,"processRules":[],"terminalProfileId":legacy});
        fixture.settings(settings);
        let preferences = fixture.import().preferences;
        assert_eq!(preferences.default_profile_id.as_str(), portable);
    }
    for profile in ["", "bad\nprofile"] {
        let fixture = Fixture::new(5);
        let settings = serde_json::json!({"version":1,"accentColor":"#123456","adapterId":"embedded-pty","shellId":"bash","shellExecutable":null,"processRules":[],"terminalProfileId":profile});
        fixture.settings(settings);
        rejected(&fixture, ErrorCode::Validation);
    }
}

#[test]
fn optional_old_settings_and_null_session_fields_use_legacy_defaults() {
    let fixture = Fixture::new(1);
    fixture.edit("UPDATE sessions SET settledAt=NULL;
        UPDATE settings SET value='{\"version\":1,\"accentColor\":\"#123456\",\"adapterId\":\"embedded-pty\",\"shellId\":\"pwsh\",\"shellExecutable\":null,\"processRules\":[],\"terminalProfileId\":null}';");
    let snapshot = fixture.import();
    assert_eq!(snapshot.sessions[0].settled_at, None);
    assert_eq!(snapshot.sessions[0].pinned_at, None);
    assert_eq!(snapshot.preferences.background_color, "#111016");
    assert_eq!(snapshot.preferences.default_profile_id.as_str(), "pwsh");
}

#[test]
fn wrong_schema_and_executable_schema_are_rejected() {
    for sql in [
        "DROP TABLE tabs",
        "ALTER TABLE sessions DROP COLUMN pinnedAt",
        "PRAGMA user_version=1",
        "ALTER TABLE sessions RENAME TO hidden; CREATE VIEW sessions AS SELECT * FROM hidden",
        "CREATE TRIGGER hostile AFTER DELETE ON sessions BEGIN DELETE FROM settings; END",
        "ALTER TABLE sessions ADD COLUMN generated TEXT GENERATED ALWAYS AS (title) VIRTUAL",
    ] {
        let fixture = Fixture::new(5);
        fixture.edit(sql);
        rejected(&fixture, ErrorCode::Validation);
    }
}

#[test]
fn corrupt_file_is_rejected_without_overwrite() {
    let fixture = Fixture::new(5);
    fs::write(fixture.source(), b"not a SQLite file: source-only-secret").unwrap();
    rejected(&fixture, ErrorCode::StorageFailed);
}

#[test]
fn imports_only_into_an_empty_destination() {
    let fixture = Fixture::new(5);
    let host = fixture.host();
    let first = host.import_legacy(fixture.source()).unwrap();
    assert_eq!(
        host.import_legacy(fixture.source()).unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(host.snapshot().unwrap(), first);
}

#[test]
fn invalid_later_row_does_not_partially_import_valid_rows() {
    let fixture = Fixture::new(5);
    fixture.edit("INSERT INTO sessions SELECT 'ffffffff-ffff-4fff-8fff-ffffffffffff','   ',cwd,adapterId,shellId,shellExecutable,createdAt,updatedAt,settledAt,error,target,env,pinnedAt FROM sessions");
    rejected(&fixture, ErrorCode::Validation);
}

#[test]
fn missing_paths_are_not_created() {
    let fixture = Fixture::new(5);
    let missing = fixture.0.join("not-created.sqlite3");
    let error = fixture.host().import_legacy(&missing).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
    assert!(!missing.exists());
}

#[test]
fn committed_wal_data_is_visible_without_checkpointing_source() {
    let fixture = Fixture::new(5);
    let writer = Connection::open(fixture.source()).unwrap();
    writer.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; UPDATE sessions SET title='Committed in WAL'").unwrap();
    let main_before = fs::read(fixture.source()).unwrap();
    let wal = fixture.0.join("source.sqlite3-wal");
    let wal_before = fs::read(&wal).unwrap();
    let snapshot = fixture.import();
    assert_eq!(snapshot.sessions[0].title, "Committed in WAL");
    assert_eq!(fs::read(fixture.source()).unwrap(), main_before);
    assert_eq!(fs::read(wal).unwrap(), wal_before);
}
