use rusqlite::Connection;
use shellfox_host::*;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

const INITIAL: &str = "2020-01-01T00:00:00.000Z";
const MARKER: &str = "2020-01-01T00:00:01.000Z";
const INPUT: &str = "2035-01-01T00:00:02.000Z";
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
// Close SQLite/ownership before removing its directory.
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
            .join(format!("activity-unavailable-{}", SessionId::new()));
        std::fs::create_dir_all(&root).unwrap();
        let host = Host::open(&root).unwrap();
        let snapshot = host
            .create_session(CreateSession {
                request_id: SessionId::new(),
                title: None,
                cwd: "/work".into(),
                profile_id: ProfileId::default(),
            })
            .unwrap();
        let session = snapshot.sessions[0].id;
        let identity = TerminalIdentity {
            tab_id: snapshot.sessions[0].tabs[0].id,
            generation: Generation::new(),
        };
        let f = Self {
            host,
            root: Scratch(root),
            session,
            identity,
        };
        f.db()
            .execute(
                "UPDATE sessions SET value=json_set(value,'$.updatedAt',?1,'$.settledAt',?2)",
                rusqlite::params![INITIAL, settled],
            )
            .unwrap();
        f.host.register_terminal(identity).unwrap();
        f
    }
    fn db(&self) -> Connection {
        Connection::open(self.root.0.join(STORE_FILENAME)).unwrap()
    }
    fn fail_writes(&self) {
        self.db().execute_batch("CREATE TRIGGER fail_activity BEFORE UPDATE ON host_state BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    }
}

#[test]
fn unavailable_never_resumes_null_or_existing_marker_but_actual_activity_still_flushes() {
    for marker in [None, Some(MARKER)] {
        let f = Fixture::new(marker);
        let captured = CapturedSettlement::Unavailable;
        let before = f.host.snapshot().unwrap();
        f.fail_writes();
        let result = f
            .host
            .resume_session_from_input(f.identity, captured.clone(), INPUT)
            .unwrap();
        assert!(!result.resumed);
        assert_eq!(result.snapshot, before);
        assert!(f.host.changed_since(before.revision).unwrap().is_none());
        assert_eq!(captured, CapturedSettlement::Unavailable);
        f.db().execute_batch("DROP TRIGGER fail_activity").unwrap();
        // Called only after positive actual input acceptance, not queue admission.
        let flushed = f.host.record_session_activity(f.session, INPUT).unwrap();
        assert_eq!(flushed.sessions[0].updated_at, INPUT);
        assert_eq!(flushed.sessions[0].settled_at.as_deref(), marker);
        assert_eq!(flushed.revision, before.revision + 1);
        let Fixture { host, root, .. } = f;
        drop(host);
        assert_eq!(Host::open(&root.0).unwrap().snapshot().unwrap(), flushed);
    }
}

#[test]
fn unavailable_queued_before_manual_mark_never_recaptures_or_changes_that_mark() {
    for initial in [None, Some(MARKER)] {
        let f = Fixture::new(initial);
        let queued = CapturedSettlement::Unavailable;
        f.host.set_session_settled(f.session, false).unwrap();
        let marked = f.host.set_session_settled(f.session, true).unwrap();
        let marker = marked.sessions[0].settled_at.clone();
        assert!(marker.is_some());
        for _ in 0..2 {
            let result = f
                .host
                .resume_session_from_input(f.identity, queued.clone(), INPUT)
                .unwrap();
            assert!(!result.resumed);
            assert_eq!(result.snapshot, marked);
        }
        let flushed = f.host.record_session_activity(f.session, INPUT).unwrap();
        assert_eq!(flushed.sessions[0].settled_at, marker);
        assert_eq!(flushed.sessions[0].updated_at, INPUT);
        assert_eq!(queued, CapturedSettlement::Unavailable);
    }
}

#[test]
fn unavailable_is_not_legacy_or_captured_null_and_legacy_behavior_stays_explicit() {
    assert_ne!(CapturedSettlement::Unavailable, CapturedSettlement::Legacy);
    assert_ne!(
        CapturedSettlement::Unavailable,
        CapturedSettlement::Captured(None)
    );
    assert_eq!(CapturedSettlement::from(None), CapturedSettlement::Legacy);
    assert_eq!(
        CapturedSettlement::from(Some(None)),
        CapturedSettlement::Captured(None)
    );
    let f = Fixture::new(Some(MARKER));
    let before = f.host.snapshot().unwrap();
    for captured in [
        CapturedSettlement::Unavailable,
        CapturedSettlement::Captured(None),
    ] {
        let result = f
            .host
            .resume_session_from_input(f.identity, captured, INPUT)
            .unwrap();
        assert!(!result.resumed);
        assert_eq!(result.snapshot, before);
    }
    let legacy = f
        .host
        .resume_session_from_input(f.identity, CapturedSettlement::Legacy, INPUT)
        .unwrap();
    assert!(legacy.resumed);
    assert!(legacy.snapshot.sessions[0].settled_at.is_none());
}

#[test]
fn unavailable_still_validates_timestamp_generation_and_prepared_cleanup() {
    let f = Fixture::new(Some(MARKER));
    let before = f.host.snapshot().unwrap();
    for bad in ["", "bad", "2035-01-01T00:00:00+02:00"] {
        assert_eq!(
            f.host
                .resume_session_from_input(f.identity, CapturedSettlement::Unavailable, bad)
                .unwrap_err()
                .code,
            ErrorCode::Validation
        );
    }
    let stale = TerminalIdentity {
        generation: Generation::new(),
        ..f.identity
    };
    assert_eq!(
        f.host
            .resume_session_from_input(stale, CapturedSettlement::Unavailable, INPUT)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(f.host.snapshot().unwrap(), before);
    for session in [false, true] {
        let pending = if session {
            f.host.prepare_delete_session(f.session).unwrap()
        } else {
            f.host.prepare_close_tab(f.identity.tab_id).unwrap()
        };
        assert_eq!(
            f.host
                .resume_session_from_input(f.identity, CapturedSettlement::Unavailable, INPUT)
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(f.host.snapshot().unwrap(), before);
        f.host.cancel_close(pending.token).unwrap();
    }
    f.host.terminal_stopped(f.identity).unwrap();
    assert_eq!(
        f.host
            .resume_session_from_input(f.identity, CapturedSettlement::Unavailable, INPUT)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(f.host.snapshot().unwrap(), before);
}

struct Guard {
    deny: AtomicBool,
    calls: AtomicUsize,
}
impl SettingsRuntime for Guard {
    fn normalize(&self, _: &Settings, candidate: Settings) -> Result<Settings> {
        Ok(candidate)
    }
    fn check_commit(&self) -> Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.deny.load(Ordering::SeqCst) {
            Err(Error::new(ErrorCode::Internal, "Native ownership lost."))
        } else {
            Ok(())
        }
    }
}

#[test]
fn unavailable_metadata_failure_keeps_original_variant_and_pending_touch_on_retry() {
    for marker in [None, Some(MARKER)] {
        let f = Fixture::new(marker);
        let captured = CapturedSettlement::Unavailable;
        let before = f.host.snapshot().unwrap();
        let guard = Arc::new(Guard {
            deny: AtomicBool::new(true),
            calls: AtomicUsize::new(0),
        });
        f.host.install_settings_runtime(guard.clone()).unwrap();
        assert_eq!(
            f.host
                .resume_session_from_input(f.identity, captured.clone(), INPUT)
                .unwrap_err()
                .code,
            ErrorCode::Internal
        );
        assert_eq!(guard.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            f.host
                .record_session_activity(f.session, INPUT)
                .unwrap_err()
                .code,
            ErrorCode::Internal
        );
        assert_eq!(f.host.snapshot().unwrap(), before);
        guard.deny.store(false, Ordering::SeqCst);
        f.fail_writes();
        assert!(
            !f.host
                .resume_session_from_input(f.identity, captured.clone(), INPUT)
                .unwrap()
                .resumed
        );
        assert_eq!(
            f.host
                .record_session_activity(f.session, INPUT)
                .unwrap_err()
                .code,
            ErrorCode::StorageFailed
        );
        assert_eq!(f.host.snapshot().unwrap(), before);
        assert_eq!(captured, CapturedSettlement::Unavailable);
        // Retry only metadata/pending touch; actual input bytes are never replayed.
        f.db().execute_batch("DROP TRIGGER fail_activity").unwrap();
        let flushed = f.host.record_session_activity(f.session, INPUT).unwrap();
        assert_eq!(flushed.sessions[0].updated_at, INPUT);
        assert_eq!(flushed.sessions[0].settled_at.as_deref(), marker);
        let retry = f
            .host
            .resume_session_from_input(f.identity, captured, INPUT)
            .unwrap();
        assert!(!retry.resumed);
        assert_eq!(retry.snapshot, flushed);
    }
}
