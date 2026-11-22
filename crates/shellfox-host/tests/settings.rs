use rusqlite::Connection;
use serde_json::{json, Value};
use shellfox_host::*;
use std::{path::PathBuf, sync::Arc};
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("settings-{}", SessionId::new()));
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
fn patch(value: Value) -> SettingsPatch {
    serde_json::from_value(value).unwrap()
}
#[test]
fn full_settings_are_durable_with_single_authoritative_persistence() {
    let dir = Scratch::new();
    let host = Host::open(&dir.0).unwrap();
    assert_eq!(host.global_settings().unwrap(), Settings::default());
    let env = vec![EnvVar {
        name: "MY_SECRET".into(),
        value: "explicit-user-value".into(),
    }];
    host.set_terminal_env(env.clone()).unwrap();
    let saved = host.save_global_settings(patch(json!({"autoUpdate":false,"accentColor":"#aabbcc","backgroundColor":"#012345","historyPageSize":31,"sessionSettleHours":1.5,"sessionDeleteHours":36,"processRules":[]}))).unwrap();
    assert_eq!(saved.terminal_env, env);
    assert!(!saved.auto_update);
    assert!(saved.process_rules.is_empty());
    let snap = host.snapshot().unwrap();
    assert_eq!(snap.preferences.accent_color, saved.accent_color);
    assert_eq!(snap.preferences.background_color, saved.background_color);
    assert_eq!(snap.revision, 2);
    assert_eq!(
        host.changed_since(1).unwrap().unwrap().reason,
        ChangeReason::Settings
    );
    host.save_global_settings(SettingsPatch::default()).unwrap();
    assert_eq!(host.snapshot().unwrap().revision, 2);
    drop(host);
    let db = dir.db();
    let state: String = db
        .query_row("SELECT value FROM host_state", [], |r| r.get(0))
        .unwrap();
    for forbidden in [
        "accentColor",
        "backgroundColor",
        "defaultProfileId",
        "terminalEnv",
        "explicit-user-value",
    ] {
        assert!(!state.contains(forbidden), "duplicated {forbidden}");
    }
    drop(db);
    let host = Host::open(&dir.0).unwrap();
    assert_eq!(host.global_settings().unwrap(), saved);
    assert_eq!(host.snapshot().unwrap(), snap);
}
#[test]
fn old_preferences_and_environment_mutations_project_to_global_settings() {
    let dir = Scratch::new();
    let host = Host::open(&dir.0).unwrap();
    host.update_preferences(PreferencesPatch {
        accent_color: Some("#ABCDEF".into()),
        default_profile_id: Some(ProfileId::new("wsl").unwrap()),
        font_size: Some(18.0),
        ..Default::default()
    })
    .unwrap();
    let settings = host.global_settings().unwrap();
    assert_eq!(settings.accent_color, "#ABCDEF");
    assert_eq!(settings.terminal_profile_id, Some(Some("wsl".into())));
    assert_eq!(settings.shell_id, SettingsShell::Wsl);
    host.save_settings(
        PreferencesPatch {
            background_color: Some("#332211".into()),
            ..Default::default()
        },
        Some(vec![EnvVar {
            name: "A".into(),
            value: "1".into(),
        }]),
    )
    .unwrap();
    let settings = host.global_settings().unwrap();
    assert_eq!(settings.background_color, "#332211");
    assert_eq!(settings.terminal_env[0].value, "1");
    assert_eq!(host.snapshot().unwrap().preferences.font_size, 18.0);
}
#[test]
fn omitted_environment_and_disjoint_concurrent_patches_do_not_lose_updates() {
    let dir = Scratch::new();
    let host = Arc::new(Host::open(&dir.0).unwrap());
    let env_host = host.clone();
    let a = std::thread::spawn(move || {
        env_host
            .set_terminal_env(vec![EnvVar {
                name: "B".into(),
                value: "2".into(),
            }])
            .unwrap()
    });
    let pref_host = host.clone();
    let b = std::thread::spawn(move || {
        pref_host
            .save_global_settings(patch(json!({"historyPageSize":43})))
            .unwrap()
    });
    a.join().unwrap();
    b.join().unwrap();
    let saved = host.global_settings().unwrap();
    assert_eq!(saved.history_page_size, 43);
    assert_eq!(saved.terminal_env[0].value, "2");
}
#[test]
fn global_settings_requests_do_not_widen_existing_snapshot() {
    let dir = Scratch::new();
    let host = Host::open(&dir.0).unwrap();
    // SettingsRequest crosses the local desktop IPC as JSON.
    let request = |input: &str| {
        serde_json::from_str::<SettingsRequest>(input)
            .ok()
            .and_then(|request| host.settings_request(request).ok())
            .is_some()
    };
    assert!(request(r#"{"method":"getGlobalSettings","payload":{}}"#));
    assert!(request(
        r#"{"method":"saveGlobalSettings","payload":{"autoUpdate":false}}"#
    ));
    assert!(!request(
        r#"{"method":"saveGlobalSettings","payload":{"terminalEnv":null}}"#
    ));
    assert!(!request(
        r#"{"method":"saveGlobalSettings","payload":{"fontSize":14}}"#
    ));
    assert!(!request(
        r#"{"method":"saveGlobalSettings","payload":{"shellExecutable":"/bin/untrusted"}}"#
    ));
    let snap = serde_json::to_value(host.snapshot().unwrap()).unwrap();
    assert!(snap.get("settings").is_none());
}
#[test]
fn failed_global_write_rolls_back_settings_projection_revision_and_notification() {
    for table in ["global_settings", "host_state"] {
        let dir = Scratch::new();
        let host = Host::open(&dir.0).unwrap();
        let before = host.global_settings().unwrap();
        let snap = host.snapshot().unwrap();
        dir.db().execute_batch(&format!("CREATE TRIGGER fail_settings BEFORE UPDATE ON {table} BEGIN SELECT RAISE(ABORT,'disk failure'); END;")).unwrap();
        assert_eq!(
            host.save_global_settings(patch(json!({"accentColor":"#123456"})))
                .unwrap_err()
                .code,
            ErrorCode::StorageFailed
        );
        assert_eq!(host.global_settings().unwrap(), before);
        assert_eq!(host.snapshot().unwrap(), snap);
        assert!(host.changed_since(0).unwrap().is_none());
    }
}
#[test]
fn every_environment_write_uses_the_same_js_trim_and_host_duplicate_policy() {
    let dir = Scratch::new();
    let host = Host::open(&dir.0).unwrap();
    let env = vec![EnvVar {
        name: "\u{feff} A \u{feff}".into(),
        value: "v".into(),
    }];
    host.set_terminal_env(env.clone()).unwrap();
    assert_eq!(host.global_settings().unwrap().terminal_env[0].name, "A");
    host.save_settings(PreferencesPatch::default(), Some(env.clone()))
        .unwrap();
    assert_eq!(host.global_settings().unwrap().terminal_env[0].name, "A");
    host.save_global_settings(SettingsPatch {
        terminal_env: Some(env.clone()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(host.global_settings().unwrap().terminal_env[0].name, "A");
    let snapshot = host
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: None,
            cwd: "/tmp/work".into(),
            profile_id: ProfileId::default(),
        })
        .unwrap();
    host.set_session_env(snapshot.sessions[0].id, env).unwrap();
    assert_eq!(host.snapshot().unwrap().sessions[0].env[0].name, "A");
    let duplicates = vec![
        EnvVar {
            name: " A ".into(),
            value: "1".into(),
        },
        EnvVar {
            name: "A".into(),
            value: "2".into(),
        },
    ];
    assert!(host.set_terminal_env(duplicates.clone()).is_err());
    assert!(host
        .set_session_env(snapshot.sessions[0].id, duplicates)
        .is_err());
    let case = vec![
        EnvVar {
            name: "x".into(),
            value: "1".into(),
        },
        EnvVar {
            name: "X".into(),
            value: "2".into(),
        },
    ];
    assert_eq!(host.set_terminal_env(case).is_err(), cfg!(windows));
}
#[test]
fn old_rust_settings_open_without_importing_the_retired_python_preference() {
    let dir = Scratch::new();
    let host = Host::open(&dir.0).unwrap();
    let settings = host.global_settings().unwrap();
    drop(host);
    let db = rusqlite::Connection::open(dir.0.join(STORE_FILENAME)).unwrap();
    db.execute(
        "UPDATE global_settings SET value=json_set(value,'$.pythonPath','/old/python')",
        [],
    )
    .unwrap();
    drop(db);
    let host = Host::open(&dir.0).unwrap();
    assert_eq!(host.global_settings().unwrap(), settings);
    let changed = host
        .save_global_settings(SettingsPatch {
            history_page_size: Some(23),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(changed.history_page_size, 23);
    let db = rusqlite::Connection::open(dir.0.join(STORE_FILENAME)).unwrap();
    let encoded: String = db
        .query_row("SELECT value FROM global_settings", [], |row| row.get(0))
        .unwrap();
    assert!(!encoded.contains("pythonPath"));
}

#[test]
fn discovery_changes_fail_closed_without_trusted_runtime_adapter() {
    let dir = Scratch::new();
    let host = Host::open(&dir.0).unwrap();
    let before = host.global_settings().unwrap();
    for input in [
        json!({"terminalProfileId":"wsl:Unavailable distro"}),
        json!({"shellExecutable":"/bin/arbitrary"}),
    ] {
        assert_eq!(
            host.save_global_settings(patch(input)).unwrap_err().code,
            ErrorCode::Unsupported
        );
        assert_eq!(host.global_settings().unwrap(), before);
    }
}
