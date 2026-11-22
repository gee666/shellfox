use super::*;

#[test]
fn imports_electron_normal_bounds_without_replacing_rust_geometry() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root).unwrap();
    let fixture = tempfile::tempdir_in(root).unwrap();
    let file = fixture.path().join("manager.sqlite3");
    let db = Connection::open(&file).unwrap();
    db.execute_batch("PRAGMA user_version=5; CREATE TABLE preferences (key TEXT PRIMARY KEY, value TEXT NOT NULL);").unwrap();
    db.execute(
        "INSERT INTO preferences VALUES (?1, ?2)",
        (
            KEY,
            r#"{"x":-1280,"y":40,"width":1170,"height":760,"maximized":true}"#,
        ),
    )
    .unwrap();
    drop(db);
    let local = fixture.path().join("native");
    let host = Host::open(&local).unwrap();
    assert_eq!(host.window_state().unwrap(), None);
    let revision = host.snapshot().unwrap().revision;
    assert!(host.import_electron_window_state(&file).unwrap());
    let state = host.window_state().unwrap().unwrap();
    assert_eq!(
        Some(state.clone()),
        WindowState::parse_electron(
            br#"{"x":-1280,"y":40,"width":1170,"height":760,"maximized":true}"#
        )
    );
    let changed = WindowState {
        x: 200,
        maximized: false,
        ..state
    };
    host.save_window_state(changed.clone()).unwrap();
    drop(host);
    let reopened = Host::open(&local).unwrap();
    assert!(!reopened.import_electron_window_state(&file).unwrap());
    assert_eq!(reopened.window_state().unwrap(), Some(changed));
    assert_eq!(reopened.snapshot().unwrap().revision, revision);
}

#[test]
fn invalid_or_corrupt_bounds_are_not_restored() {
    let db = Connection::open_in_memory().unwrap();
    migrate(&db).unwrap();
    for malformed in [
        "null",
        "{",
        r#"{"x":0,"y":0,"width":99,"height":760,"maximized":true}"#,
    ] {
        db.execute(
            "INSERT OR REPLACE INTO desktop_preferences VALUES (?1, ?2)",
            (KEY, malformed),
        )
        .unwrap();
        assert_eq!(read(&db, "desktop_preferences", WindowState::parse).unwrap(), None);
    }
}
