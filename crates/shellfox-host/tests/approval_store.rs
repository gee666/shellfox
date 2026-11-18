use rusqlite::Connection;
use shellfox_host::*;
use std::path::PathBuf;
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!("approval-sqlite-{}", SessionId::new()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn db(&self) -> Connection {
        Connection::open(self.0.join(STORE_FILENAME)).unwrap()
    }
    fn v4(&self) {
        self.db().execute_batch("DROP TABLE native_approvals; DROP TABLE native_approval_state; PRAGMA user_version=4;").unwrap();
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn record(peer: &str) -> ApprovalStoreRecord {
    ApprovalStoreRecord {
        peer_id: peer.into(),
        hostname: "owned".into(),
        approved_at_millis: 1,
        policy: ApprovalStorePolicy::ControlFullAccountV1,
    }
}
#[test]
fn v4_migration_preserves_metadata_settings_facts_and_request_receipts() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let input = CreateSession {
        request_id: SessionId::new(),
        title: Some("preserved".into()),
        cwd: "/owned".into(),
        profile_id: ProfileId::new("cmd").unwrap(),
    };
    host.create_session_with_facts(
        input.clone(),
        SessionCreationFacts::new(SettingsAdapter::EmbeddedPty, SettingsShell::LoginShell),
    )
    .unwrap();
    let before = host.snapshot().unwrap();
    let settings = host.global_settings().unwrap();
    let facts = host.session_creation_facts().unwrap();
    let receipt = host.creation_request_receipt(input.request_id).unwrap();
    drop(host);
    scratch.v4();
    let host = Host::open(&scratch.0).unwrap();
    assert_eq!(host.snapshot().unwrap(), before);
    assert_eq!(host.global_settings().unwrap(), settings);
    assert_eq!(host.session_creation_facts().unwrap(), facts);
    assert_eq!(
        host.creation_request_receipt(input.request_id).unwrap(),
        receipt
    );
    assert_eq!(host.create_session(input).unwrap(), before);
    assert_eq!(
        host.approval_snapshot().unwrap(),
        ApprovalStoreSnapshot {
            revision: 0,
            approvals: vec![]
        }
    );
    drop(host);
    assert_eq!(
        scratch
            .db()
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        5
    );
}
#[test]
fn committed_approvals_revoke_and_restart_use_exact_revision_cas() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let committed = host.commit_approvals(0, vec![record("n-owned")]).unwrap();
    assert_eq!(committed.revision, 1);
    drop(host);
    let host = Host::open(&scratch.0).unwrap();
    assert_eq!(host.approval_snapshot().unwrap(), committed);
    assert_eq!(
        host.commit_approvals(0, vec![]).unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(host.approval_snapshot().unwrap(), committed);
    let revoked = host.commit_approvals(1, vec![]).unwrap();
    drop(host);
    let host = Host::open(&scratch.0).unwrap();
    assert_eq!(host.approval_snapshot().unwrap(), revoked);
    assert!(revoked.approvals.is_empty());
}
#[test]
fn v5_missing_corrupt_unknown_or_pending_auth_never_gains_empty_fallback() {
    for mutation in [
        "DELETE FROM native_approval_state",
        "DROP TABLE native_approval_state",
        "DROP TABLE native_approvals",
        "PRAGMA ignore_check_constraints=ON; UPDATE native_approval_state SET feature_version=2",
        "PRAGMA ignore_check_constraints=ON; UPDATE native_approval_state SET policy='future-policy'",
        "UPDATE native_approval_state SET phase='pending'",
        "ALTER TABLE native_approvals ADD COLUMN future_mode TEXT",
        "INSERT INTO native_approvals VALUES('n-owned','owned',1,'control-full-account-v1'); UPDATE native_approvals SET peer_id=char(27)",
    ] {
        let scratch = Scratch::new(); drop(Host::open(&scratch.0).unwrap());
        scratch.db().execute_batch(mutation).unwrap();
        let host = Host::open(&scratch.0).unwrap();
        assert!(host.approval_snapshot().is_err(), "accepted {mutation}");
        assert!(host.commit_approvals(0, vec![]).is_err(), "repaired {mutation}");
        drop(host);
        assert_eq!(scratch.db().pragma_query_value(None,"user_version", |r| r.get::<_,i64>(0)).unwrap(),5);
    }
}
fn deferred_commit_fault(db: &Connection, phase: &str) {
    db.execute_batch(&format!("CREATE TABLE fault_parent(id INTEGER PRIMARY KEY);
        CREATE TABLE fault_child(id INTEGER REFERENCES fault_parent(id) DEFERRABLE INITIALLY DEFERRED);
        CREATE TRIGGER approval_commit_fault AFTER UPDATE OF phase ON native_approval_state
        WHEN NEW.phase='{phase}' BEGIN INSERT INTO fault_child VALUES(42); END;")).unwrap();
}
#[test]
fn actual_final_commit_failure_preserves_pending_intent_and_denies_restart() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    host.commit_approvals(0, vec![record("n-owned")]).unwrap();
    let metadata = host.snapshot().unwrap();
    drop(host);
    deferred_commit_fault(&scratch.db(), "committed");
    let host = Host::open(&scratch.0).unwrap();
    assert!(host.commit_approvals(1, vec![]).is_err());
    assert!(host.approval_snapshot().is_err());
    assert_eq!(host.snapshot().unwrap(), metadata);
    drop(host);
    assert_eq!(
        scratch
            .db()
            .query_row("SELECT phase FROM native_approval_state", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "pending"
    );
    let host = Host::open(&scratch.0).unwrap();
    assert!(host.approval_snapshot().is_err());
}
#[test]
fn failed_initial_intent_is_not_a_durable_revocation_receipt() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let original = host.commit_approvals(0, vec![record("n-owned")]).unwrap();
    drop(host);
    deferred_commit_fault(&scratch.db(), "pending");
    let host = Host::open(&scratch.0).unwrap();
    assert!(host.commit_approvals(1, vec![]).is_err());
    assert_eq!(host.approval_snapshot().unwrap(), original);
}
#[test]
fn unexpected_auth_objects_or_invalid_v4_data_roll_back_whole_migration() {
    for corrupt in [
        "CREATE TABLE native_approval_state(foreign_value TEXT)",
        "UPDATE host_state SET value='{}'",
    ] {
        let scratch = Scratch::new();
        drop(Host::open(&scratch.0).unwrap());
        scratch.v4();
        scratch.db().execute_batch(corrupt).unwrap();
        assert!(Host::open(&scratch.0).is_err());
        let db = scratch.db();
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            4
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name='native_approvals'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
}
