//! The journal: the Host answers from memory while its disk writes trail
//! behind in order, and a disk that fails delays durability but loses nothing.
use rusqlite::Connection;
use shellfox_host::*;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("journal-{}", SessionId::new()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn db(&self) -> Connection {
        Connection::open(self.0.join(STORE_FILENAME)).unwrap()
    }
    fn block_disk(&self) {
        self.db()
            .execute_batch("CREATE TRIGGER stalled BEFORE UPDATE ON host_state BEGIN SELECT RAISE(ABORT,'disk stalled'); END;")
            .unwrap();
    }
    fn unblock_disk(&self) {
        self.db().execute_batch("DROP TRIGGER stalled").unwrap();
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default, Clone)]
struct Seen(Arc<Mutex<(Vec<u64>, Vec<u64>)>>);
impl Seen {
    fn hooks(&self) -> JournalHooks {
        let committed = self.clone();
        let durable = self.clone();
        JournalHooks {
            committed: Arc::new(move |seq| committed.0.lock().unwrap().0.push(seq)),
            durable: Arc::new(move |seq| durable.0.lock().unwrap().1.push(seq)),
        }
    }
}

fn session(host: &Host, title: &str) -> Snapshot {
    host.create_session(CreateSession {
        request_id: SessionId::new(),
        title: Some(title.into()),
        cwd: "/work".into(),
        profile_id: ProfileId::default(),
    })
    .unwrap()
}

#[test]
fn write_through_commits_on_disk_before_it_returns() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    session(&host, "one");
    assert_eq!(host.written().unwrap(), 1);
    assert_eq!(host.durable().unwrap(), 1);
    let titles: i64 = scratch
        .db()
        .query_row("SELECT count(*) FROM sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(titles, 1);
}

#[test]
fn a_journaled_write_answers_while_the_disk_fails_and_lands_once_it_recovers() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let seen = Seen::default();
    host.start_journal(seen.hooks()).unwrap();
    scratch.block_disk();
    let first = session(&host, "first");
    let second = session(&host, "second");
    // The memory copy answers at once; nothing reached the disk.
    assert_eq!(host.snapshot().unwrap(), second);
    assert_eq!(host.written().unwrap(), 2);
    assert!(host.flush(Duration::from_millis(300)).is_err());
    assert_eq!(host.durable().unwrap(), 0);
    assert!(host.durability_failure().unwrap().is_some());
    assert_eq!(seen.0.lock().unwrap().0, [1, 2]);
    scratch.unblock_disk();
    host.flush(Duration::from_secs(10)).unwrap();
    assert_eq!(host.durable().unwrap(), 2);
    assert!(host.durability_failure().unwrap().is_none());
    assert_eq!(seen.0.lock().unwrap().1.last(), Some(&2));
    assert!(first.revision < second.revision);
    drop(host);
    assert_eq!(Host::open(&scratch.0).unwrap().snapshot().unwrap(), second);
}

#[test]
fn dropping_a_journaled_host_waits_until_its_changes_are_on_disk() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    host.start_journal(Seen::default().hooks()).unwrap();
    scratch.block_disk();
    let snapshot = session(&host, "kept");
    let unblock = {
        let path = scratch.0.join(STORE_FILENAME);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            Connection::open(path)
                .unwrap()
                .execute_batch("DROP TRIGGER stalled")
                .unwrap();
        })
    };
    drop(host);
    unblock.join().unwrap();
    assert_eq!(
        Host::open(&scratch.0).unwrap().snapshot().unwrap(),
        snapshot
    );
}

/// Reordering, cascading deletes, settings, approvals and device preferences
/// replay on disk to the same store the memory copy holds.
#[test]
fn the_disk_replays_every_kind_of_change_to_the_same_store() {
    for journaled in [false, true] {
        let scratch = Scratch::new();
        let host = Host::open(&scratch.0).unwrap();
        if journaled {
            host.start_journal(Seen::default().hooks()).unwrap();
        }
        let snapshot = session(&host, "kept");
        let id = snapshot.sessions[0].id;
        for _ in 0..3 {
            host.add_tab(
                id,
                AddTab {
                    title: None,
                    cwd: None,
                    profile_id: ProfileId::default(),
                },
            )
            .unwrap();
        }
        let tabs: Vec<_> = host.snapshot().unwrap().sessions[0]
            .tabs
            .iter()
            .rev()
            .map(|tab| tab.id)
            .collect();
        host.reorder_tabs(id, &tabs).unwrap();
        host.close_tab(tabs[1]).unwrap();
        let gone = session(&host, "gone")
            .sessions
            .iter()
            .find(|s| s.title == "gone")
            .unwrap()
            .id;
        host.delete_session(gone).unwrap();
        host.rename_session(id, "renamed").unwrap();
        host.save_global_settings(SettingsPatch {
            history_page_size: Some(42),
            ..Default::default()
        })
        .unwrap();
        let approvals = host
            .commit_approvals(
                0,
                vec![ApprovalStoreRecord {
                    peer_id: "n-peer".into(),
                    hostname: "peer".into(),
                    approved_at_millis: 1,
                    policy: ApprovalStorePolicy::ControlFullAccountV1,
                }],
            )
            .unwrap();
        let window = shellfox_core::window_state::WindowState::parse(
            br#"{"x":10,"y":20,"width":800,"height":600,"maximized":false}"#,
        );
        assert!(window.is_some());
        if let Some(window) = window.clone() {
            host.save_window_state(window).unwrap();
        }
        let expected = host.snapshot().unwrap();
        let settings = host.global_settings().unwrap();
        host.flush(Duration::from_secs(10)).unwrap();
        drop(host);
        let reopened = Host::open(&scratch.0).unwrap();
        assert_eq!(
            reopened.snapshot().unwrap(),
            expected,
            "journaled: {journaled}"
        );
        assert_eq!(reopened.global_settings().unwrap(), settings);
        assert_eq!(reopened.approval_snapshot().unwrap(), approvals);
        assert_eq!(reopened.window_state().unwrap(), window);
        assert_eq!(expected.sessions.len(), 1);
        assert_eq!(expected.sessions[0].tabs.len(), 3);
    }
}
