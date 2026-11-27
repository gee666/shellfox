use rusqlite::Connection;
use shellfox_host::*;
use std::{path::PathBuf, sync::Arc};
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("manual-settlement-{}", SessionId::new()));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn db(&self) -> Connection {
        Connection::open(self.0.join(STORE_FILENAME)).unwrap()
    }
    fn live(&self) -> (Host, SessionId, TerminalIdentity) {
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
        host.register_terminal(identity).unwrap();
        (host, session, identity)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn live_manual_settle_resettle_protects_new_marker_from_stale_or_null_captured_input() {
    let scratch = Scratch::new();
    let (host, session, identity) = scratch.live();
    let first = host.set_session_settled(session, true).unwrap();
    let old = first.sessions[0].settled_at.clone().unwrap();
    let repeated = host.set_session_settled(session, true).unwrap();
    assert_eq!(repeated.sessions[0].settled_at, Some(old.clone()));
    assert_eq!(repeated.revision, first.revision + 1);
    host.set_session_settled(session, false).unwrap();
    let settled = host.set_session_settled(session, true).unwrap();
    let marker = settled.sessions[0].settled_at.clone().unwrap();
    assert!(activity_unix_ms(&marker).unwrap() > activity_unix_ms(&old).unwrap());
    let at = activity_utc_timestamp(activity_unix_ms(&marker).unwrap() + 1000).unwrap();
    for captured in [
        CapturedSettlement::Captured(None),
        CapturedSettlement::Captured(Some(old)),
    ] {
        let result = host
            .resume_session_from_input(identity, captured, &at)
            .unwrap();
        assert!(!result.resumed);
        assert_eq!(result.snapshot, settled);
    }
    let stale = TerminalIdentity {
        generation: Generation::new(),
        ..identity
    };
    assert_eq!(
        host.resume_session_from_input(stale, CapturedSettlement::Legacy, &at)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    let resumed = host
        .resume_session_from_input(identity, CapturedSettlement::Captured(Some(marker)), &at)
        .unwrap();
    assert!(resumed.resumed);
    assert_eq!(resumed.snapshot.sessions[0].settled_at, None);
    // Manual flags and resumption did not unregister or clean up any process owner.
    let prepared = host.prepare_close_tab(identity.tab_id).unwrap();
    assert_eq!(prepared.terminals, vec![identity]);
    assert_eq!(
        host.commit_close(prepared.token).unwrap_err().code,
        ErrorCode::CleanupRequired
    );
    host.cancel_close(prepared.token).unwrap();
    host.set_session_settled(session, true).unwrap();
    assert_eq!(
        host.delete_session(session).unwrap_err().code,
        ErrorCode::CleanupRequired
    );
}
#[test]
fn prepared_close_blocks_manual_settle_and_unsettle_even_for_another_tab_in_the_session() {
    for whole_session in [false, true] {
        let scratch = Scratch::new();
        let (host, session, identity) = scratch.live();
        let added = host
            .add_tab(
                session,
                AddTab {
                    title: None,
                    cwd: None,
                    profile_id: ProfileId::default(),
                },
            )
            .unwrap();
        host.set_session_settled(session, true).unwrap();
        let prepared = if whole_session {
            host.prepare_delete_session(session).unwrap()
        } else {
            host.prepare_close_tab(added.sessions[0].tabs[1].id)
                .unwrap()
        };
        let before = host.snapshot().unwrap();
        for settled in [true, false] {
            assert_eq!(
                host.set_session_settled(session, settled).unwrap_err().code,
                ErrorCode::Conflict
            );
            assert_eq!(host.snapshot().unwrap(), before);
        }
        host.cancel_close(prepared.token).unwrap();
        let marker = before.sessions[0].settled_at.clone().unwrap();
        assert!(
            host.resume_session_from_input(
                identity,
                CapturedSettlement::Captured(Some(marker)),
                &before.sessions[0].updated_at
            )
            .unwrap()
            .resumed
        );
    }
}
#[test]
fn explicit_manual_actions_touch_updated_at_and_revision_without_replacing_existing_marker() {
    let scratch = Scratch::new();
    let (host, session, _) = scratch.live();
    let first = host.set_session_settled(session, true).unwrap();
    let marker = first.sessions[0].settled_at.clone();
    scratch
        .db()
        .execute_batch(
            "UPDATE sessions SET value=json_set(value,'$.updatedAt','2000-01-01T00:00:00.000Z')",
        )
        .unwrap();
    let repeated = host.set_session_settled(session, true).unwrap();
    assert_eq!(repeated.sessions[0].settled_at, marker);
    assert_ne!(repeated.sessions[0].updated_at, "2000-01-01T00:00:00.000Z");
    assert_eq!(repeated.revision, first.revision + 1);
    let unset = host.set_session_settled(session, false).unwrap();
    let repeated = host.set_session_settled(session, false).unwrap();
    assert_eq!(repeated.sessions[0].settled_at, None);
    assert_eq!(repeated.revision, unset.revision + 1);
    drop(host);
    assert_eq!(
        Host::open(&scratch.0).unwrap().snapshot().unwrap(),
        repeated
    );
}
#[test]
fn persisted_marker_seeds_clock_before_another_operation_clears_it_after_reopen() {
    let scratch = Scratch::new();
    let (host, session, identity) = scratch.live();
    let prior = "2050-01-01T00:00:00.000Z";
    scratch
        .db()
        .execute(
            "UPDATE sessions SET value=json_set(value,'$.settledAt',?)",
            [prior],
        )
        .unwrap();
    drop(host);
    let host = Host::open(&scratch.0).unwrap();
    // Adding a tab keeps the marker (0.9); an explicit unsettle clears it.
    host.set_session_settled(session, false).unwrap();
    let current = TerminalIdentity {
        generation: Generation::new(),
        ..identity
    };
    host.register_terminal(current).unwrap();
    let settled = host.set_session_settled(session, true).unwrap();
    let marker = settled.sessions[0].settled_at.clone().unwrap();
    assert!(activity_unix_ms(&marker).unwrap() > activity_unix_ms(prior).unwrap());
    assert_eq!(
        host.resume_session_from_input(identity, CapturedSettlement::Legacy, &marker)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert!(
        !host
            .resume_session_from_input(
                current,
                CapturedSettlement::Captured(Some(prior.into())),
                &marker
            )
            .unwrap()
            .resumed
    );
}
struct Lost;
impl SettingsRuntime for Lost {
    fn normalize(&self, _: &Settings, candidate: Settings) -> Result<Settings> {
        Ok(candidate)
    }
    fn check_commit(&self) -> Result<()> {
        Err(Error::new(ErrorCode::Internal, "Native ownership lost."))
    }
}
#[test]
fn failed_manual_commit_keeps_live_registration_flag_and_revision_unchanged() {
    let scratch = Scratch::new();
    let (host, session, identity) = scratch.live();
    let before = host.snapshot().unwrap();
    host.install_settings_runtime(Arc::new(Lost)).unwrap();
    for settled in [true, false] {
        assert_eq!(
            host.set_session_settled(session, settled).unwrap_err().code,
            ErrorCode::Internal
        );
        assert_eq!(host.snapshot().unwrap(), before);
    }
    let prepared = host.prepare_close_tab(identity.tab_id).unwrap();
    assert_eq!(prepared.terminals, vec![identity]);
    host.cancel_close(prepared.token).unwrap();
}

#[test]
fn idle_settling_keeps_the_last_activity_time() {
    let scratch = Scratch::new();
    let (host, session, identity) = scratch.live();
    let before = host.snapshot().unwrap();
    let updated = before.sessions[0].updated_at.clone();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let settled = host.settle_idle_session(session).unwrap();
    let marker = settled.sessions[0].settled_at.clone().unwrap();
    assert_eq!(settled.sessions[0].updated_at, updated);
    assert_eq!(settled.revision, before.revision + 1);
    // Settling again changes nothing.
    let again = host.settle_idle_session(session).unwrap();
    assert_eq!(again.sessions[0].settled_at, Some(marker.clone()));
    assert_eq!(again.revision, settled.revision);
    // Input resumes it like a manual settle.
    let at = activity_utc_timestamp(activity_unix_ms(&marker).unwrap() + 1000).unwrap();
    let resumed = host
        .resume_session_from_input(identity, CapturedSettlement::Captured(Some(marker)), &at)
        .unwrap();
    assert!(resumed.resumed);
}
