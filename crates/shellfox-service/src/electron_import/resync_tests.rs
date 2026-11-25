//! Import, Rust changes, 0.9 changes, Rust again: nothing lost or duplicated.
use super::{
    tests::{policy, settings, Fixture},
    *,
};
use rusqlite::Connection;
use shellfox_core::{CreateSession, ProfileId, SessionId, SettingsPatch};
use std::{collections::BTreeMap, fs};

const SESSION: &str = "f919fb1a-fb03-4a93-8b9b-1cde465d5870";
/// Deleted in Rust, untouched in Electron: stays deleted.
const DELETED: &str = "0b5c2b80-6c5e-4a61-9a43-3c2b7f0e1d01";
/// Deleted in Rust, then used in Electron: comes back.
const REVIVED: &str = "0b5c2b80-6c5e-4a61-9a43-3c2b7f0e1d02";
/// Renamed in Rust after Electron's change: Rust's name stays.
const RUST_NEWER: &str = "0b5c2b80-6c5e-4a61-9a43-3c2b7f0e1d03";
/// Created in Electron after the import.
const NEW: &str = "0b5c2b80-6c5e-4a61-9a43-3c2b7f0e1d04";

fn now() -> String {
    shellfox_host::activity_utc_timestamp(now_ms()).unwrap()
}

fn electron(fixture: &Fixture) -> Connection {
    Connection::open(fixture.electron.join(files::DATABASE)).unwrap()
}

fn add(db: &Connection, id: &str, title: &str, at: &str) {
    db.execute(
        "INSERT INTO sessions (id,title,cwd,adapterId,shellId,shellExecutable,createdAt,updatedAt,env)
         VALUES (?1,?2,'/home/user/other','embedded-pty','bash','/bin/bash',?3,?3,'[]')",
        [id, title, at],
    )
    .unwrap();
}

fn rename(db: &Connection, id: &str, title: &str, at: &str) {
    db.execute(
        "UPDATE sessions SET title=?2, updatedAt=?3 WHERE id=?1",
        [id, title, at],
    )
    .unwrap();
}

fn titles(fixture: &Fixture) -> BTreeMap<String, String> {
    let sessions = Host::open(&fixture.data)
        .unwrap()
        .snapshot()
        .unwrap()
        .sessions;
    let titles: BTreeMap<_, _> = sessions
        .iter()
        .map(|s| (s.id.to_string(), s.title.clone()))
        .collect();
    assert_eq!(titles.len(), sessions.len(), "duplicate session");
    titles
}

#[test]
fn a_round_trip_through_electron_loses_and_duplicates_nothing() {
    let fixture = Fixture::new();
    fixture.database(&settings());
    let db = electron(&fixture);
    for (id, title) in [
        (DELETED, "Deleted"),
        (REVIVED, "Revived"),
        (RUST_NEWER, "Old"),
    ] {
        add(&db, id, title, "2025-03-01T00:00:00.000Z");
    }
    drop(db);
    assert_eq!(fixture.run(false).unwrap().unwrap().sessions, 4);
    drop(retention::take_held(&fixture.electron));
    let imported_at = now();

    // Rust: rename, delete two, create one, change one setting.
    std::thread::sleep(std::time::Duration::from_millis(20));
    let host = Host::open(&fixture.data).unwrap();
    let id = |text: &str| text.parse::<SessionId>().unwrap();
    host.rename_session(id(SESSION), "Rust name").unwrap();
    host.delete_session(id(DELETED)).unwrap();
    host.delete_session(id(REVIVED)).unwrap();
    let created = host
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: Some("Rust only".into()),
            cwd: fixture.data.to_string_lossy().into_owned(),
            profile_id: ProfileId::default(),
        })
        .unwrap();
    let rust_only = created.sessions.iter().find(|s| s.title == "Rust only");
    let rust_only = rust_only.unwrap().id.to_string();
    host.save_global_settings(SettingsPatch {
        history_page_size: Some(7),
        ..Default::default()
    })
    .unwrap();
    // Electron's change below is older than this one.
    std::thread::sleep(std::time::Duration::from_millis(20));
    host.rename_session(id(RUST_NEWER), "Rust newer").unwrap();
    drop(host);

    // 0.9 again: a new session, a rename, a revived session, an accent color.
    std::thread::sleep(std::time::Duration::from_millis(20));
    let db = electron(&fixture);
    add(&db, NEW, "Made in 0.9", &now());
    rename(&db, SESSION, "Electron name", &now());
    rename(&db, REVIVED, "Used in 0.9", &now());
    rename(&db, RUST_NEWER, "Electron older", &imported_at);
    let mut changed = settings();
    changed.accent_color = "#ff0000".into();
    db.execute(
        "UPDATE settings SET value=?1",
        [serde_json::to_string(&changed).unwrap()],
    )
    .unwrap();
    drop(db);
    let profile = serde_json::json!({"version":1,"profiles":[{
        "id":"e64ec926-9af9-43dd-9221-084847d93ffe","name":"Build VM","host":"vm.example","port":2222,
        "user":"build","password":"fixture-secret","keyFile":null,"remoteCwd":"/work","source":"putty",
        "connectionOptions":{"addressFamily":4,"tryAgent":false,"agentForward":false}}]});
    fs::write(fixture.electron.join(SSH), profile.to_string()).unwrap();

    // Rust again.
    assert!(fixture.run(false).unwrap().is_none());
    let expected = BTreeMap::from([
        (SESSION.to_owned(), "Electron name".to_owned()),
        (REVIVED.to_owned(), "Used in 0.9".to_owned()),
        (RUST_NEWER.to_owned(), "Rust newer".to_owned()),
        (NEW.to_owned(), "Made in 0.9".to_owned()),
        (rust_only, "Rust only".to_owned()),
    ]);
    assert_eq!(titles(&fixture), expected);
    let host = Host::open(&fixture.data).unwrap();
    let merged = host.global_settings().unwrap();
    assert_eq!(
        (merged.accent_color.as_str(), merged.history_page_size),
        ("#ff0000", 7)
    );
    drop(host);
    let profiles = ProfileStore::new(Arc::new(
        PrivateDirectory::open(fixture.control.join("ssh"), policy()).unwrap(),
    ));
    assert_eq!(profiles.list().unwrap().len(), 1);

    // Later Rust changes survive further launches; nothing merges twice.
    Host::open(&fixture.data)
        .unwrap()
        .rename_session(id(NEW), "Renamed in Rust")
        .unwrap();
    assert!(fixture.run(false).unwrap().is_none());
    let mut expected = expected;
    expected.insert(NEW.to_owned(), "Renamed in Rust".to_owned());
    assert_eq!(titles(&fixture), expected);
}

#[test]
fn an_unchanged_electron_is_not_read_again() {
    let fixture = Fixture::new();
    fixture.database(&settings());
    fixture.run(false).unwrap().unwrap();
    drop(retention::take_held(&fixture.electron));
    let sync = fixture.control.join("electron-sync-v1.json");
    let recorded = fs::read(&sync).unwrap();
    // A running Electron would block a merge; an unchanged one is never locked.
    fs::create_dir(fixture.electron.join(".host-lock")).unwrap();
    let lock = fs::File::create(fixture.electron.join(".host-lock/host.lock")).unwrap();
    lock.lock().unwrap();
    assert!(fixture.run(false).unwrap().is_none());
    assert_eq!(fs::read(&sync).unwrap(), recorded);
    lock.unlock().unwrap();
}
