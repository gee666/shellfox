use rusqlite::Connection;
use shellfox_host::*;
use std::path::PathBuf;
const INITIAL: &str = "2030-01-01T00:00:00.900Z";
const INPUT: &str = "2030-01-01T00:00:02.000Z";
const OLD: &str = "2030-01-01T00:00:01.000Z";
const NEW: &str = "2030-01-01T00:00:01.001Z";
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
// Fields drop in declaration order: close SQLite/ownership before deleting files.
struct Fixture {
    host: Host,
    root: Scratch,
    session: SessionId,
    identity: TerminalIdentity,
}
impl Fixture {
    fn new(settled: Option<&str>) -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("activity-store-{}", SessionId::new()));
        std::fs::create_dir_all(&root).unwrap();
        let host = Host::open(&root).unwrap();
        let snap = host
            .create_session(CreateSession {
                request_id: SessionId::new(),
                title: None,
                cwd: "/work".into(),
                profile_id: ProfileId::default(),
            })
            .unwrap();
        let session = snap.sessions[0].id;
        let identity = TerminalIdentity {
            tab_id: snap.sessions[0].tabs[0].id,
            generation: Generation::new(),
        };
        let fixture = Self {
            root: Scratch(root),
            host,
            session,
            identity,
        };
        fixture.times(INITIAL, settled);
        fixture.host.register_terminal(identity).unwrap();
        fixture
    }
    fn db(&self) -> Connection {
        Connection::open(self.root.0.join(STORE_FILENAME)).unwrap()
    }
    fn times(&self, at: &str, settled: Option<&str>) {
        self.host.test_write("UPDATE sessions SET value=json_set(value,'$.updatedAt',?1,'$.settledAt',?2) WHERE id=?3",rusqlite::params![at,settled,self.session.to_string()]).unwrap();
    }
    fn block_writes(&self) {
        self.db().execute_batch("CREATE TRIGGER fail_activity BEFORE UPDATE ON host_state BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    }
    fn snapshot(&self) -> Snapshot {
        self.host.snapshot().unwrap()
    }
}
fn cleanup(f: Fixture) {
    drop(f);
}
#[test]
fn batched_activity_is_monotonic_by_utc_instant_durable_and_noop_for_older_or_equal() {
    let f = Fixture::new(Some(NEW));
    f.times("2030-01-01T00:00:00.9Z", Some(NEW));
    let before = f.snapshot();
    f.block_writes();
    for at in [INITIAL, "2030-01-01T00:00:00.800Z"] {
        assert_eq!(
            f.host.record_session_activity(f.session, at).unwrap(),
            before
        );
    }
    assert_eq!(
        f.host
            .record_session_activity(f.session, INPUT)
            .unwrap_err()
            .code,
        ErrorCode::StorageFailed
    );
    assert_eq!(f.snapshot(), before);
    f.db().execute_batch("DROP TRIGGER fail_activity").unwrap();
    let saved = f.host.record_session_activity(f.session, INPUT).unwrap();
    assert_eq!(saved.sessions[0].updated_at, INPUT);
    assert_eq!(saved.sessions[0].settled_at.as_deref(), Some(NEW));
    assert_eq!(saved.revision, before.revision + 1);
    assert_eq!(
        f.host
            .changed_since(before.revision)
            .unwrap()
            .unwrap()
            .reason,
        ChangeReason::Sessions
    );
    let Fixture { host, root, .. } = f;
    drop(host);
    let reopened = Host::open(&root.0).unwrap();
    assert_eq!(reopened.snapshot().unwrap(), saved);
    drop(reopened);
    drop(root);
}
#[test]
fn matching_capture_clears_once_and_subsequent_keys_do_not_write_or_retime_activity() {
    let f = Fixture::new(Some(OLD));
    let before = f.snapshot();
    let result = f
        .host
        .resume_session_from_input(
            f.identity,
            CapturedSettlement::Captured(Some(OLD.into())),
            INPUT,
        )
        .unwrap();
    assert!(result.resumed);
    assert_eq!(result.snapshot.sessions[0].settled_at, None);
    assert_eq!(result.snapshot.sessions[0].updated_at, INPUT);
    assert_eq!(result.snapshot.revision, before.revision + 1);
    f.block_writes();
    for captured in [
        CapturedSettlement::Legacy,
        CapturedSettlement::Captured(None),
        CapturedSettlement::Captured(Some(OLD.into())),
    ] {
        let result = f
            .host
            .resume_session_from_input(f.identity, captured, "2030-01-01T00:00:03.000Z")
            .unwrap();
        assert!(!result.resumed);
        assert_eq!(result.snapshot.sessions[0].updated_at, INPUT);
    }
    cleanup(f);
}
#[test]
fn stale_and_captured_null_input_preserve_new_manual_settlement_and_debounce_touch() {
    for captured in [
        CapturedSettlement::Captured(None),
        CapturedSettlement::Captured(Some(OLD.into())),
    ] {
        let f = Fixture::new(Some(NEW));
        let before = f.snapshot();
        f.block_writes();
        let result = f
            .host
            .resume_session_from_input(f.identity, captured, INPUT)
            .unwrap();
        assert!(!result.resumed);
        assert_eq!(result.snapshot, before);
        // The accepted PTY input's pending owner touch is retained independently.
        f.db().execute_batch("DROP TRIGGER fail_activity").unwrap();
        let flushed = f.host.record_session_activity(f.session, INPUT).unwrap();
        assert_eq!(flushed.sessions[0].updated_at, INPUT);
        assert_eq!(flushed.sessions[0].settled_at.as_deref(), Some(NEW));
        cleanup(f);
    }
}
#[test]
fn legacy_input_explicitly_clears_current_marker_but_never_moves_activity_backwards() {
    let f = Fixture::new(Some(NEW));
    f.times(INPUT, Some(NEW));
    let result = f
        .host
        .resume_session_from_input(f.identity, CapturedSettlement::Legacy, OLD)
        .unwrap();
    assert!(result.resumed);
    assert_eq!(result.snapshot.sessions[0].settled_at, None);
    assert_eq!(result.snapshot.sessions[0].updated_at, INPUT);
    cleanup(f);
}
#[test]
fn timestamp_marker_cas_is_exact_not_same_instant_or_session_time_substitution() {
    let f = Fixture::new(Some("2030-01-01T00:00:01Z"));
    let before = f.snapshot();
    let result = f
        .host
        .resume_session_from_input(
            f.identity,
            CapturedSettlement::Captured(Some(OLD.into())),
            INPUT,
        )
        .unwrap();
    assert!(!result.resumed);
    assert_eq!(result.snapshot, before);
    cleanup(f);
}
#[test]
fn stale_runtime_generation_cannot_clear_or_persist_an_input_touch() {
    let f = Fixture::new(Some(OLD));
    let before = f.snapshot();
    let stale = TerminalIdentity {
        generation: Generation::new(),
        ..f.identity
    };
    assert_eq!(
        f.host
            .resume_session_from_input(stale, CapturedSettlement::Legacy, INPUT)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(f.snapshot(), before);
    f.host.terminal_stopped(f.identity).unwrap();
    let replacement = TerminalIdentity {
        generation: Generation::new(),
        ..f.identity
    };
    f.host.register_terminal(replacement).unwrap();
    assert_eq!(
        f.host
            .resume_session_from_input(f.identity, CapturedSettlement::Legacy, INPUT)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert!(
        f.host
            .resume_session_from_input(replacement, CapturedSettlement::Legacy, INPUT)
            .unwrap()
            .resumed
    );
    cleanup(f);
}
#[test]
fn prepared_tab_or_session_cleanup_blocks_activity_and_resume_without_changing_token_revision() {
    for entire_session in [false, true] {
        let f = Fixture::new(Some(OLD));
        let snapshot = f
            .host
            .add_tab(
                f.session,
                AddTab {
                    title: None,
                    cwd: None,
                    profile_id: ProfileId::default(),
                },
            )
            .unwrap();
        let prepared = if entire_session {
            f.host.prepare_delete_session(f.session).unwrap()
        } else {
            f.host
                .prepare_close_tab(snapshot.sessions[0].tabs[1].id)
                .unwrap()
        };
        let before = f.snapshot();
        assert_eq!(
            f.host
                .record_session_activity(f.session, INPUT)
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(
            f.host
                .resume_session_from_input(f.identity, CapturedSettlement::Legacy, INPUT)
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(f.snapshot(), before);
        f.host.cancel_close(prepared.token).unwrap();
        f.host.record_session_activity(f.session, INPUT).unwrap();
        cleanup(f);
    }
}
#[test]
fn invalid_utc_touches_and_markers_fail_without_mutation() {
    let f = Fixture::new(Some(OLD));
    let before = f.snapshot();
    for bad in ["", "not UTC", "2030-01-01T00:00:00+02:00"] {
        assert_eq!(
            f.host
                .record_session_activity(f.session, bad)
                .unwrap_err()
                .code,
            ErrorCode::Validation
        );
        assert_eq!(
            f.host
                .resume_session_from_input(f.identity, CapturedSettlement::Legacy, bad)
                .unwrap_err()
                .code,
            ErrorCode::Validation
        );
        assert_eq!(
            f.host
                .resume_session_from_input(
                    f.identity,
                    CapturedSettlement::Captured(Some(bad.into())),
                    INPUT
                )
                .unwrap_err()
                .code,
            ErrorCode::Validation
        );
    }
    assert_eq!(f.snapshot(), before);
    cleanup(f);
}
#[test]
fn omitted_null_and_timestamp_capture_mapping_stays_distinct() {
    assert_eq!(CapturedSettlement::from(None), CapturedSettlement::Legacy);
    assert_eq!(
        CapturedSettlement::from(Some(None)),
        CapturedSettlement::Captured(None)
    );
    assert_eq!(
        CapturedSettlement::from(Some(Some(OLD.into()))),
        CapturedSettlement::Captured(Some(OLD.into()))
    );
}
