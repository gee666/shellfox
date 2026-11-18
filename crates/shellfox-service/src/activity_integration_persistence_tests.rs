use super::*;
use shellfox_core::{CreateSession, Generation, ProfileId};
use std::path::PathBuf;
struct Store {
    host: Host,
    identity: Identity,
    settled: Option<String>,
    _scratch: tempfile::TempDir,
}
fn store(settle: bool) -> Store {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
    std::fs::create_dir_all(&root).unwrap();
    let scratch = tempfile::Builder::new()
        .prefix("activity-store-")
        .tempdir_in(root)
        .unwrap();
    let host = Host::open(scratch.path()).unwrap();
    let snapshot = host
        .create_session(CreateSession {
            request_id: SessionId::new(),
            title: Some("activity fixture".into()),
            cwd: scratch.path().to_string_lossy().into_owned(),
            profile_id: ProfileId::new("cmd").unwrap(),
        })
        .unwrap();
    let identity = Identity {
        session_id: snapshot.selection.session_id.unwrap(),
        tab_id: snapshot.selection.tab_id.unwrap(),
        generation: Generation::new(),
    };
    let settled = if settle {
        host.set_session_settled(identity.session_id, true)
            .unwrap()
            .sessions[0]
            .settled_at
            .clone()
    } else {
        None
    };
    host.register_terminal(TerminalIdentity {
        tab_id: identity.tab_id,
        generation: identity.generation,
    })
    .unwrap();
    Store {
        host,
        identity,
        settled,
        _scratch: scratch,
    }
}
fn pending() -> ActivityPersistence {
    ActivityPersistence::new(ActivityUtcAnchor {
        logical_ms: 1000,
        unix_ms: shellfox_host::activity_unix_ms("2050-01-01T00:00:00.000Z").unwrap(),
    })
}
fn input() -> InputActivity {
    InputActivity {
        touch_session: true,
        unsettle: false,
    }
}

#[test]
fn accepted_input_cas_writes_only_actual_transition_and_legacy_missing_remains_explicit() {
    let store = store(true);
    let mut pending = pending();
    let revision = store.host.snapshot().unwrap().revision;
    let captured_null = pending.accepted_input(
        &store.host,
        store.identity,
        input(),
        CapturedSettlement::Captured(None),
        1100,
    );
    assert!(!captured_null.resumed);
    assert!(captured_null.metadata_error.is_none());
    assert_eq!(pending.pending_count(), 1);
    assert_eq!(store.host.snapshot().unwrap().revision, revision);
    assert_eq!(
        store.host.snapshot().unwrap().sessions[0].settled_at,
        store.settled
    );
    let matched = pending.accepted_input(
        &store.host,
        store.identity,
        input(),
        CapturedSettlement::Captured(store.settled.clone()),
        1200,
    );
    assert!(matched.resumed);
    assert!(matched.metadata_error.is_none());
    assert_eq!(
        matched.snapshot.unwrap().sessions[0].updated_at,
        "2050-01-01T00:00:00.200Z"
    );
    assert_eq!(pending.pending_count(), 0);
    let legacy = self::store(true);
    let mut pending = self::pending();
    assert!(
        pending
            .accepted_input(
                &legacy.host,
                legacy.identity,
                input(),
                CapturedSettlement::Legacy,
                1100
            )
            .resumed
    );
}
#[test]
fn minute_flush_uses_updated_at_and_passive_input_does_not_create_a_touch() {
    let store = store(false);
    let mut pending = pending();
    let revision = store.host.snapshot().unwrap().revision;
    let passive = pending.accepted_input(
        &store.host,
        store.identity,
        InputActivity::default(),
        CapturedSettlement::Legacy,
        1100,
    );
    assert!(passive.snapshot.is_none());
    assert_eq!(pending.pending_count(), 0);
    pending.accepted_input(
        &store.host,
        store.identity,
        input(),
        CapturedSettlement::Captured(None),
        1100,
    );
    assert_eq!(store.host.snapshot().unwrap().revision, revision);
    assert!(matches!(
        pending.flush_next(&store.host, 60_999, false),
        ActivityFlush::Idle
    ));
    assert!(matches!(
        pending.flush_next(&store.host, 61_000, false),
        ActivityFlush::Committed { .. }
    ));
    assert_eq!(
        store.host.snapshot().unwrap().sessions[0].updated_at,
        "2050-01-01T00:00:00.100Z"
    );
    assert_eq!(pending.pending_count(), 0);
}
#[test]
fn prepared_close_metadata_failure_retains_touch_without_replaying_or_failing_the_write() {
    let store = store(true);
    let mut pending = pending();
    let prepared = store.host.prepare_close_tab(store.identity.tab_id).unwrap();
    let accepted = pending.accepted_input(
        &store.host,
        store.identity,
        input(),
        CapturedSettlement::Legacy,
        1100,
    );
    assert_eq!(
        accepted.metadata_error,
        Some(ActivityPersistenceFailure::Storage)
    );
    assert!(!accepted.resumed);
    assert_eq!(pending.pending_count(), 1);
    assert!(matches!(
        pending.flush_next(&store.host, 1200, true),
        ActivityFlush::Deferred { .. }
    ));
    assert_eq!(pending.pending_count(), 1);
    store.host.cancel_close(prepared.token).unwrap();
    assert!(matches!(
        pending.flush_next(&store.host, 1300, true),
        ActivityFlush::Committed { .. }
    ));
    assert_eq!(pending.pending_count(), 0);
    assert_eq!(
        store.host.snapshot().unwrap().sessions[0].settled_at,
        store.settled
    );
}
#[test]
fn newer_pending_touch_survives_an_earlier_successful_commit_and_stale_generation_is_metadata_only()
{
    let store = store(true);
    let mut pending = pending();
    pending.touch(store.identity.session_id, 1100).unwrap();
    store
        .host
        .record_session_activity(store.identity.session_id, "2050-01-01T00:00:00.100Z")
        .unwrap();
    pending.touch(store.identity.session_id, 1200).unwrap();
    pending.committed(store.identity.session_id, 1100);
    assert_eq!(pending.pending_count(), 1);
    assert!(matches!(
        pending.flush_next(&store.host, 1300, true),
        ActivityFlush::Committed { .. }
    ));
    assert_eq!(
        store.host.snapshot().unwrap().sessions[0].updated_at,
        "2050-01-01T00:00:00.200Z"
    );
    let stale = Identity {
        generation: Generation::new(),
        ..store.identity
    };
    let accepted = pending.accepted_input(
        &store.host,
        stale,
        input(),
        CapturedSettlement::Legacy,
        1400,
    );
    assert_eq!(
        accepted.metadata_error,
        Some(ActivityPersistenceFailure::Storage)
    );
    assert_eq!(pending.pending_count(), 1);
    assert_eq!(
        store.host.snapshot().unwrap().sessions[0].settled_at,
        store.settled
    );
    pending.forget_session(store.identity.session_id);
    assert_eq!(pending.pending_count(), 0);
}
#[test]
fn live_echo_and_idle_touches_do_not_depend_on_a_meaningful_timestamp() {
    let store = store(false);
    let mut pending = pending();
    pending
        .live_updates(&[LiveActivityUpdate {
            identity: store.identity,
            last_sequence: 1,
            output_at_ms: Some(1100),
            meaningful_at_ms: None,
            busy_changed: None,
            available: true,
        }])
        .unwrap();
    assert_eq!(pending.pending_count(), 1);
    pending.flush_next(&store.host, 1300, true);
    assert_eq!(
        store.host.snapshot().unwrap().sessions[0].updated_at,
        "2050-01-01T00:00:00.100Z"
    );
}
