use rusqlite::Connection;
use shellfox_host::*;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
const MARKER: &str = "2030-01-01T00:00:01.000Z";
const AT: &str = "2030-01-01T00:00:02.000Z";
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("activity-commit-{}", SessionId::new()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn db(&self) -> Connection {
        Connection::open(self.0.join(STORE_FILENAME)).unwrap()
    }
    fn host(&self) -> (Host, SessionId, TerminalIdentity) {
        let host = Host::open(&self.0).unwrap();
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
        host.test_write("UPDATE sessions SET value=json_set(value,'$.settledAt',?,'$.updatedAt','2030-01-01T00:00:00.000Z')",[MARKER]).unwrap();
        host.register_terminal(identity).unwrap();
        (host, session, identity)
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
        Err(Error::new(ErrorCode::Internal, "Native ownership lost."))
    }
}
#[test]
fn failed_resume_sql_commit_does_not_clear_marker_or_acknowledge_pending_touch() {
    let scratch = Scratch::new();
    let (host, session, identity) = scratch.host();
    let before = host.snapshot().unwrap();
    scratch.db().execute_batch("CREATE TRIGGER fail_activity BEFORE UPDATE ON host_state BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    // This call is metadata-only, AFTER the owner's PTY write already succeeded.
    // Retain the pending touch on Err; never retry/replay the accepted input bytes.
    let pending = AT;
    assert_eq!(
        host.resume_session_from_input(
            identity,
            CapturedSettlement::Captured(Some(MARKER.into())),
            pending
        )
        .unwrap_err()
        .code,
        ErrorCode::StorageFailed
    );
    assert_eq!(host.snapshot().unwrap(), before);
    assert!(host.changed_since(before.revision).unwrap().is_none());
    scratch
        .db()
        .execute_batch("DROP TRIGGER fail_activity")
        .unwrap();
    let flushed = host.record_session_activity(session, pending).unwrap();
    assert_eq!(flushed.sessions[0].settled_at.as_deref(), Some(MARKER));
    assert_eq!(flushed.sessions[0].updated_at, AT);
    drop(host);
    assert_eq!(Host::open(&scratch.0).unwrap().snapshot().unwrap(), flushed);
}
#[test]
fn late_ownership_loss_rolls_back_activity_and_resume_with_pending_touches_unacknowledged() {
    for resume in [false, true] {
        let scratch = Scratch::new();
        let (host, session, identity) = scratch.host();
        let before = host.snapshot().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        host.install_settings_runtime(Arc::new(DenyCommit(calls.clone())))
            .unwrap();
        let result = if resume {
            host.resume_session_from_input(identity, CapturedSettlement::Legacy, AT)
                .map(|_| ())
        } else {
            host.record_session_activity(session, AT).map(|_| ())
        };
        assert_eq!(result.unwrap_err().code, ErrorCode::Internal);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(host.snapshot().unwrap(), before);
        assert!(host.changed_since(before.revision).unwrap().is_none());
        drop(host);
        assert_eq!(Host::open(&scratch.0).unwrap().snapshot().unwrap(), before);
    }
}
#[test]
fn accepted_input_with_no_matching_marker_has_no_write_but_still_rechecks_ownership() {
    let scratch = Scratch::new();
    let (host, _, identity) = scratch.host();
    let before = host.snapshot().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    host.install_settings_runtime(Arc::new(DenyCommit(calls.clone())))
        .unwrap();
    assert_eq!(
        host.resume_session_from_input(identity, CapturedSettlement::Captured(None), AT)
            .unwrap_err()
            .code,
        ErrorCode::Internal
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(host.snapshot().unwrap(), before);
}
