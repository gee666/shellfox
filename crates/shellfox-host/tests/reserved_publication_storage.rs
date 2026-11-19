use shellfox_host::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
#[path = "support/reserved_publication.rs"]
mod support;
use support::*;

#[test]
fn sql_failure_rolls_back_ids_facts_receipt_revision_and_selection_then_retries_exactly() {
    for (table, operation) in [
        ("sessions", "INSERT"),
        ("tabs", "INSERT"),
        ("session_creation_facts", "INSERT"),
        ("host_state", "UPDATE"),
        ("reserved_publications", "INSERT"),
    ] {
        let scratch = Scratch::new();
        let host = Host::open(&scratch.0).unwrap();
        host.create_session(ordinary(SessionId::new())).unwrap();
        let before = host.snapshot().unwrap();
        let request = input();
        scratch.db().execute_batch(&format!("CREATE TRIGGER fail_publication BEFORE {operation} ON {table} BEGIN SELECT RAISE(ABORT,'injected failure'); END;")).unwrap();
        assert_eq!(
            host.publish_reserved_session(request.clone(), Some(facts()))
                .unwrap_err()
                .code,
            ErrorCode::StorageFailed
        );
        assert_eq!(host.snapshot().unwrap(), before);
        assert_eq!(
            host.reserved_publication_receipt(request.request_id)
                .unwrap(),
            ReservedPublicationReceipt::ReceiptUnavailable
        );
        assert!(host.session_creation_facts().unwrap().is_empty());
        assert!(host.changed_since(before.revision).unwrap().is_none());
        drop(host);
        scratch
            .db()
            .execute_batch("DROP TRIGGER fail_publication")
            .unwrap();
        let host = Host::open(&scratch.0).unwrap();
        assert_eq!(host.snapshot().unwrap(), before);
        let saved = host
            .publish_reserved_session(request.clone(), Some(facts()))
            .unwrap();
        assert_eq!(saved.revision, before.revision + 1);
        assert_eq!(saved.selection, before.selection);
        assert_eq!(
            host.reserved_publication_receipt(request.request_id)
                .unwrap(),
            ReservedPublicationReceipt::Published {
                session_id: request.session_id,
                tab_id: request.tab_id
            }
        );
        assert_eq!(
            host.publish_reserved_session(request, Some(facts()))
                .unwrap(),
            saved
        );
    }
}

struct Guard(AtomicBool);
impl SettingsRuntime for Guard {
    fn normalize(&self, _: &Settings, candidate: Settings) -> Result<Settings> {
        Ok(candidate)
    }
    fn check_commit(&self) -> Result<()> {
        if self.0.load(Ordering::SeqCst) {
            Err(Error::new(ErrorCode::Internal, "Native ownership lost."))
        } else {
            Ok(())
        }
    }
}
#[test]
fn late_ownership_loss_rolls_back_and_retry_rechecks_guard_without_registering_runtime() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let guard = Arc::new(Guard(AtomicBool::new(true)));
    host.install_settings_runtime(guard.clone()).unwrap();
    let before = host.snapshot().unwrap();
    let request = input();
    assert_eq!(
        host.publish_reserved_session(request.clone(), Some(facts()))
            .unwrap_err()
            .code,
        ErrorCode::Internal
    );
    assert_eq!(host.snapshot().unwrap(), before);
    assert!(host.session_creation_facts().unwrap().is_empty());
    assert_eq!(
        host.reserved_publication_receipt(request.request_id)
            .unwrap(),
        ReservedPublicationReceipt::ReceiptUnavailable
    );
    guard.0.store(false, Ordering::SeqCst);
    let saved = host
        .publish_reserved_session(request.clone(), Some(facts()))
        .unwrap();
    guard.0.store(true, Ordering::SeqCst);
    assert_eq!(
        host.publish_reserved_session(request.clone(), Some(facts()))
            .unwrap_err()
            .code,
        ErrorCode::Internal
    );
    assert_eq!(host.snapshot().unwrap(), saved);
    let prepared = host.prepare_delete_session(request.session_id).unwrap();
    assert!(prepared.terminals.is_empty());
    host.cancel_close(prepared.token).unwrap();
    guard.0.store(false, Ordering::SeqCst);
    assert_eq!(
        host.publish_reserved_session(request, Some(facts()))
            .unwrap(),
        saved
    );
}

#[test]
fn version_three_migration_keeps_old_receipt_session_only_and_selection_unchanged() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let request_id = SessionId::new();
    let before = host
        .create_session_with_facts(ordinary(request_id), facts())
        .unwrap();
    let saved_facts = host.session_creation_facts().unwrap();
    drop(host);
    scratch
        .db()
        .execute_batch("DROP TABLE native_approvals; DROP TABLE native_approval_state; DROP TABLE reserved_publications; DROP TABLE desktop_preferences; PRAGMA user_version=3;")
        .unwrap();
    let host = Host::open(&scratch.0).unwrap();
    assert_eq!(host.snapshot().unwrap(), before);
    assert_eq!(host.session_creation_facts().unwrap(), saved_facts);
    assert_eq!(
        host.creation_request_receipt(request_id).unwrap(),
        CreationRequestReceipt::SessionCreated {
            session_id: before.sessions[0].id
        }
    );
    assert_eq!(
        host.reserved_publication_receipt(request_id).unwrap(),
        ReservedPublicationReceipt::ReceiptUnavailable
    );
    let mut collision = input();
    collision.request_id = request_id;
    assert_eq!(
        host.publish_reserved_session(collision, Some(facts()))
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        scratch
            .db()
            .pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        6
    );
    let request = input();
    let saved = host
        .publish_reserved_session(request.clone(), Some(facts()))
        .unwrap();
    drop(host);
    let host = Host::open(&scratch.0).unwrap();
    assert_eq!(
        host.publish_reserved_session(request, Some(facts()))
            .unwrap(),
        saved
    );
}

#[test]
fn unknown_or_known_original_facts_never_change_on_retry_including_after_reopen() {
    for original in [None, Some(facts())] {
        let scratch = Scratch::new();
        let host = Host::open(&scratch.0).unwrap();
        let request = input();
        let saved = host
            .publish_reserved_session(request.clone(), original.clone())
            .unwrap();
        assert_eq!(
            host.session_creation_facts()
                .unwrap()
                .get(&request.session_id),
            original.as_ref()
        );
        assert_eq!(saved.selection, Selection::default());
        drop(host);
        let host = Host::open(&scratch.0).unwrap();
        let different =
            SessionCreationFacts::new(SettingsAdapter::GnomeTerminal, SettingsShell::Bash);
        for retry in [None, Some(different)] {
            assert_eq!(
                host.publish_reserved_session(request.clone(), retry)
                    .unwrap(),
                saved
            );
            assert_eq!(
                host.session_creation_facts()
                    .unwrap()
                    .get(&request.session_id),
                original.as_ref()
            );
        }
        assert_eq!(
            host.reserved_publication_receipt(request.request_id)
                .unwrap(),
            ReservedPublicationReceipt::Published {
                session_id: request.session_id,
                tab_id: request.tab_id
            }
        );
        host.delete_session(request.session_id).unwrap();
        assert_eq!(
            host.publish_reserved_session(request, None)
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
    }
}

#[test]
fn no_facts_publication_rolls_back_receipt_failure_and_retries_without_backfill() {
    let scratch = Scratch::new();
    let host = Host::open(&scratch.0).unwrap();
    let request = input();
    let before = host.snapshot().unwrap();
    scratch.db().execute_batch("CREATE TRIGGER fail_publication BEFORE INSERT ON reserved_publications BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    assert_eq!(
        host.publish_reserved_session(request.clone(), None)
            .unwrap_err()
            .code,
        ErrorCode::StorageFailed
    );
    assert_eq!(host.snapshot().unwrap(), before);
    assert_eq!(
        host.reserved_publication_receipt(request.request_id)
            .unwrap(),
        ReservedPublicationReceipt::ReceiptUnavailable
    );
    assert!(host.session_creation_facts().unwrap().is_empty());
    scratch
        .db()
        .execute_batch("DROP TRIGGER fail_publication")
        .unwrap();
    let saved = host
        .publish_reserved_session(request.clone(), None)
        .unwrap();
    assert_eq!(
        host.publish_reserved_session(request, Some(facts()))
            .unwrap(),
        saved
    );
    assert!(host.session_creation_facts().unwrap().is_empty());
}

#[test]
fn corrupt_version_three_migration_rolls_back_receipt_table_and_schema_version() {
    let scratch = Scratch::new();
    drop(Host::open(&scratch.0).unwrap());
    scratch.db().execute_batch("DROP TABLE native_approvals; DROP TABLE native_approval_state; DROP TABLE reserved_publications; DROP TABLE desktop_preferences; PRAGMA user_version=3; UPDATE host_state SET value='{}';").unwrap();
    assert_eq!(
        Host::open(&scratch.0).err().unwrap().code,
        ErrorCode::StorageFailed
    );
    let db = scratch.db();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='reserved_publications'",
            [],
            |r| r.get::<_, u32>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn corrupt_or_missing_receipt_table_is_not_defaulted_or_repaired() {
    for corruption in [
        "PRAGMA ignore_check_constraints=ON;UPDATE reserved_publications SET version=2",
        "UPDATE reserved_publications SET tab_id='invalid'",
        "UPDATE reserved_publications SET request_id=upper(request_id)",
        "UPDATE session_creation_facts SET value='{}'",
        "INSERT INTO requests SELECT request_id,session_id FROM reserved_publications",
        "DROP TABLE reserved_publications",
    ] {
        let scratch = Scratch::new();
        let host = Host::open(&scratch.0).unwrap();
        host.publish_reserved_session(input(), Some(facts()))
            .unwrap();
        drop(host);
        scratch.db().execute_batch(corruption).unwrap();
        assert_eq!(
            Host::open(&scratch.0).err().unwrap().code,
            ErrorCode::StorageFailed,
            "{corruption}"
        );
    }
}
