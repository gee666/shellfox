use rusqlite::Connection;
use shellfox_host::*;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("settings-commit-{}", SessionId::new()));
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
struct DenyCommit(Arc<AtomicUsize>);
impl SettingsRuntime for DenyCommit {
    fn normalize(&self, _: &Settings, candidate: Settings) -> Result<Settings> {
        Ok(candidate)
    }
    fn check_commit(&self) -> Result<()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(Error::new(
            ErrorCode::Internal,
            "Native ownership was lost before commit.",
        ))
    }
}
fn mutate(host: &Host, route: &str) -> Result<()> {
    match route {
        "global" => host
            .save_global_settings(SettingsPatch {
                accent_color: Some("#123456".into()),
                ..Default::default()
            })
            .map(|_| ()),
        "preferences" => host
            .update_preferences(PreferencesPatch {
                accent_color: Some("#123456".into()),
                ..Default::default()
            })
            .map(|_| ()),
        "environment" => host
            .set_terminal_env(vec![EnvVar {
                name: "USER_VAR".into(),
                value: "explicit".into(),
            }])
            .map(|_| ()),
        "compatibility" => host
            .save_settings(
                PreferencesPatch {
                    background_color: Some("#123456".into()),
                    ..Default::default()
                },
                None,
            )
            .map(|_| ()),
        _ => unreachable!("fixed test route"),
    }
}
#[test]
fn all_settings_write_routes_roll_back_the_final_guard_failure() {
    for route in ["global", "preferences", "environment", "compatibility"] {
        let scratch = Scratch::new();
        let host = Host::open(&scratch.0).unwrap();
        let before = host.global_settings().unwrap();
        let snapshot = host.snapshot().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        host.install_settings_runtime(Arc::new(DenyCommit(calls.clone())))
            .unwrap();
        assert_eq!(
            mutate(&host, route).unwrap_err().code,
            ErrorCode::Internal,
            "{route}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(host.global_settings().unwrap(), before);
        assert_eq!(host.snapshot().unwrap(), snapshot);
        assert!(host.changed_since(0).unwrap().is_none());
        drop(host);
        let reopened = Host::open(&scratch.0).unwrap();
        assert_eq!(reopened.global_settings().unwrap(), before);
        assert_eq!(reopened.snapshot().unwrap(), snapshot);
    }
}
#[test]
fn sql_writes_must_finish_before_the_commit_guard_is_called() {
    for route in ["global", "environment"] {
        let scratch = Scratch::new();
        let host = Host::open(&scratch.0).unwrap();
        let before = host.global_settings().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        host.install_settings_runtime(Arc::new(DenyCommit(calls.clone())))
            .unwrap();
        scratch.db().execute_batch("CREATE TRIGGER reject_sql BEFORE UPDATE ON host_state BEGIN SELECT RAISE(ABORT,'injected write failure'); END;").unwrap();
        // A guard mistakenly moved before SQL would return Internal instead.
        assert_eq!(
            mutate(&host, route).unwrap_err().code,
            ErrorCode::StorageFailed
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(host.global_settings().unwrap(), before);
        assert_eq!(host.snapshot().unwrap().revision, 0);
    }
}
#[test]
fn settings_noop_still_checks_native_ownership_before_read_transaction_commit() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    host.install_settings_runtime(Arc::new(DenyCommit(calls.clone())))
        .unwrap();
    assert_eq!(
        host.save_global_settings(SettingsPatch::default())
            .unwrap_err()
            .code,
        ErrorCode::Internal
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(host.snapshot().unwrap().revision, 0);
}
#[test]
fn copy_import_rolls_back_settings_and_sessions_if_native_guard_is_lost() {
    let scratch = Scratch::new();
    let source = scratch.0.join("copy.sqlite3");
    let db = Connection::open(&source).unwrap();
    db.execute_batch(include_str!("fixtures/legacy-v1.sql"))
        .unwrap();
    drop(db);
    let bytes = std::fs::read(&source).unwrap();
    let host = Host::open(&scratch.0).unwrap();
    let before = host.global_settings().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    host.install_settings_runtime(Arc::new(DenyCommit(calls.clone())))
        .unwrap();
    assert_eq!(
        host.import_legacy(&source).unwrap_err().code,
        ErrorCode::Internal
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(host.global_settings().unwrap(), before);
    assert!(host.snapshot().unwrap().sessions.is_empty());
    assert_eq!(host.snapshot().unwrap().revision, 0);
    assert_eq!(std::fs::read(source).unwrap(), bytes);
}
