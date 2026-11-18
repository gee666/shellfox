use rusqlite::Connection;
use serde_json::json;
use shellfox_host::*;
use std::path::PathBuf;
struct Scratch(PathBuf);
impl Scratch {
    fn new(version: i32) -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("settings-import-{}", SessionId::new()));
        std::fs::create_dir_all(&path).unwrap();
        let dir = Self(path);
        let db = dir.source_db();
        db.execute_batch(include_str!("fixtures/legacy-v1.sql"))
            .unwrap();
        if version >= 2 {
            db.execute_batch("CREATE TABLE terminal_metadata (tabId TEXT PRIMARY KEY REFERENCES tabs(id),value TEXT NOT NULL);").unwrap();
        }
        if version >= 3 {
            db.execute_batch("ALTER TABLE sessions ADD COLUMN env TEXT NOT NULL DEFAULT '[]'; UPDATE sessions SET env='[{\"name\":\"SESSION_SECRET\",\"value\":\"not-imported\"}]';").unwrap();
        }
        if version >= 4 {
            db.execute_batch("ALTER TABLE sessions ADD COLUMN pinnedAt TEXT;")
                .unwrap();
        }
        db.pragma_update(None, "user_version", version).unwrap();
        dir
    }
    fn source(&self) -> PathBuf {
        self.0.join("copy.sqlite3")
    }
    fn source_db(&self) -> Connection {
        Connection::open(self.source()).unwrap()
    }
    fn config(&self) -> Settings {
        let mut config = Settings {
            auto_update: false,
            python_path: Some("/old/system/python3".into()),
            accent_color: "#AaBB01".into(),
            background_color: "#012345".into(),
            adapter_id: SettingsAdapter::EmbeddedPty,
            shell_id: SettingsShell::Wsl,
            shell_executable: Some("C:\\Windows\\System32\\wsl.exe".into()),
            terminal_profile_id: Some(Some("wsl:Unavailable opaque distro (dev)".into())),
            history_page_size: 87,
            session_settle_hours: 1.5,
            session_delete_hours: 2.75,
            terminal_env: vec![EnvVar {
                name: "GLOBAL_SECRET".into(),
                value: "explicit-consent-only".into(),
            }],
            ..Default::default()
        };
        config.process_rules[0].enabled = false;
        config.process_rules.push(SettingsProcessRule {
            id: "custom-local-tool-123".into(),
            label: "My tool".into(),
            enabled: true,
            executable_basenames: vec![],
            executable_paths: vec!["/opt/my-tool".into()],
            script_path_suffixes: vec![],
            process_names: None,
        });
        self.source_db()
            .execute(
                "UPDATE settings SET value=?",
                [serde_json::to_string(&config).unwrap()],
            )
            .unwrap();
        config
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn all_five_copy_versions_preserve_user_configuration_but_not_runtime_or_secrets_by_default() {
    for version in 1..=5 {
        let dir = Scratch::new(version);
        let mut expected = dir.config();
        let before = std::fs::read(dir.source()).unwrap();
        let host = Host::open(dir.0.join("destination")).unwrap();
        let snap = host.import_legacy(dir.source()).unwrap();
        expected.terminal_env.clear();
        assert_eq!(
            host.global_settings().unwrap(),
            expected,
            "version {version}"
        );
        assert_eq!(
            snap.preferences.default_profile_id.as_str(),
            "wsl:Unavailable opaque distro (dev)"
        );
        assert!(snap
            .sessions
            .iter()
            .all(|s| s.tabs.is_empty() && s.env.is_empty()));
        let persisted = std::fs::read(dir.0.join("destination").join(STORE_FILENAME)).unwrap();
        let text = String::from_utf8_lossy(&persisted);
        for excluded in [
            "explicit-consent-only",
            "SESSION_SECRET",
            "not-imported",
            "987654321",
            "source-only-secret",
        ] {
            assert!(!text.contains(excluded));
        }
        assert_eq!(std::fs::read(dir.source()).unwrap(), before);
        let saved = host.global_settings().unwrap();
        drop(host);
        assert_eq!(
            Host::open(dir.0.join("destination"))
                .unwrap()
                .global_settings()
                .unwrap(),
            saved
        );
    }
}
#[test]
fn global_environment_requires_explicit_consent_and_is_not_session_runtime_adoption() {
    let dir = Scratch::new(5);
    let expected = dir.config();
    let host = Host::open(dir.0.join("consented")).unwrap();
    let snapshot = host
        .import_legacy_with_settings(dir.source(), SettingsImportPolicy::IncludeGlobalEnvironment)
        .unwrap();
    assert_eq!(host.global_settings().unwrap(), expected);
    assert!(snapshot
        .sessions
        .iter()
        .all(|s| s.tabs.is_empty() && s.env.is_empty()));
    assert_eq!(snapshot.revision, 1);
    assert_eq!(
        host.changed_since(0).unwrap().unwrap().reason,
        ChangeReason::Settings
    );
}
#[test]
fn unknown_private_credentials_are_not_silently_accepted_into_global_settings() {
    let dir = Scratch::new(5);
    let config = dir.config();
    let mut raw = serde_json::to_value(config).unwrap();
    raw["password"] = json!("never-persist-this");
    dir.source_db()
        .execute("UPDATE settings SET value=?", [raw.to_string()])
        .unwrap();
    let host = Host::open(dir.0.join("destination")).unwrap();
    let before = host.global_settings().unwrap();
    assert_eq!(
        host.import_legacy(dir.source()).unwrap_err().code,
        ErrorCode::Validation
    );
    assert_eq!(host.global_settings().unwrap(), before);
    assert_eq!(host.snapshot().unwrap().revision, 0);
}
#[test]
fn explicit_environment_consent_does_not_bypass_validation() {
    let dir = Scratch::new(5);
    let config = dir.config();
    let mut raw = serde_json::to_value(config).unwrap();
    raw["terminalEnv"] = json!([{"name":"A","value":"old\ninvalid"}]);
    dir.source_db()
        .execute("UPDATE settings SET value=?", [raw.to_string()])
        .unwrap();
    let host = Host::open(dir.0.join("destination")).unwrap();
    assert_eq!(
        host.import_legacy_with_settings(
            dir.source(),
            SettingsImportPolicy::IncludeGlobalEnvironment
        )
        .unwrap_err()
        .code,
        ErrorCode::Validation
    );
    assert_eq!(host.snapshot().unwrap().revision, 0);
    host.import_legacy(dir.source()).unwrap();
    assert!(host.global_settings().unwrap().terminal_env.is_empty());
}
