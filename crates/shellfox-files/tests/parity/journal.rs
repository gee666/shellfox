use crate::support::Fixture;
use shellfox_files::*;
use std::{
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;
fn open(f: &Fixture) -> TransferRegistry {
    let mut registry = TransferRegistry::new(TransferLimits::default()).unwrap();
    registry.initialize_journal(&f.metadata).unwrap();
    registry
}
fn document(f: &Fixture) -> PathBuf {
    f.metadata.join("file-transfer-stages-v2.json")
}
fn unknown(f: &Fixture) -> PathBuf {
    f.root
        .join(format!(".shellfox-transfer-{}.partial", Uuid::new_v4()))
}
fn row(path: &Path) -> serde_json::Value {
    serde_json::json!({"stage":path.to_str().unwrap(),"target":"diagnostic only","phase":"owned"})
}
#[test]
fn startup_requires_v2_and_exclusive_lease_never_reaps_a_live_stage() {
    let f = Fixture::new();
    let actor = super::streams::actor();
    let cap = super::streams::capability(&f);
    let mut first = TransferRegistry::new(TransferLimits::default()).unwrap();
    assert_eq!(
        first
            .begin_write(
                &actor,
                cap.clone(),
                &super::streams::create("new", 0),
                &super::streams::allow
            )
            .unwrap_err()
            .code,
        ErrorCode::StorageFailed
    );
    first.initialize_journal(&f.metadata).unwrap();
    let start = first
        .begin_write(
            &actor,
            cap,
            &super::streams::create("new", 0),
            &super::streams::allow,
        )
        .unwrap();
    let mut second = TransferRegistry::new(TransferLimits::default()).unwrap();
    assert!(second.initialize_journal(&f.metadata).is_err());
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 1);
    first
        .finish_write(
            &actor,
            &TransferIdentity {
                transfer_id: start.transfer_id,
            },
            &super::streams::allow,
        )
        .unwrap();
    first.dispose();
    let next = f.metadata.join(".file-transfer-stages.next");
    fs::write(&next, b"unknown legacy next").unwrap();
    second.initialize_journal(&f.metadata).unwrap();
    assert_eq!(fs::read(next).unwrap(), b"unknown legacy next");
    assert_eq!(fs::read(f.root.join("new")).unwrap(), b"");
}
#[test]
fn v2_restart_records_do_not_reconstruct_original_user_stage_authority() {
    let f = Fixture::new();
    drop(open(&f));
    let named = unknown(&f);
    let unrelated = unknown(&f);
    fs::write(&named, b"old draft").unwrap();
    fs::write(&unrelated, b"not journaled").unwrap();
    let bytes =
        serde_json::to_vec(&serde_json::json!({"version":2,"entries":[row(&named)]})).unwrap();
    fs::write(document(&f), &bytes).unwrap();
    let mut registry = open(&f);
    registry.sweep(&super::streams::allow).unwrap();
    drop(registry);
    assert_eq!(fs::read(&named).unwrap(), b"old draft");
    assert_eq!(fs::read(&unrelated).unwrap(), b"not journaled");
    assert_eq!(fs::read(document(&f)).unwrap(), bytes);
}
#[test]
fn legacy_v1_next_and_lease_bytes_are_preserved_without_parsing_or_recovery() {
    let f = Fixture::new();
    drop(open(&f));
    let old = f.metadata.join("file-transfer-stages.json");
    let next = f.metadata.join(".file-transfer-stages.next");
    let lease = f.metadata.join(".file-transfer-stages.lock");
    let data = unknown(&f);
    fs::write(&data, b"unrelated").unwrap();
    for (path, bytes) in [
        (&old, b"unknown V1 document".as_slice()),
        (&next, b"next"),
        (&lease, b"lease"),
    ] {
        fs::write(path, bytes).unwrap();
    }
    let _registry = open(&f);
    for (path, bytes) in [
        (&old, b"unknown V1 document".as_slice()),
        (&next, b"next"),
        (&lease, b"lease"),
        (&data, b"unrelated"),
    ] {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}
#[test]
fn invalid_or_unsupported_v2_schema_is_not_repaired_and_does_not_touch_user_data() {
    let f = Fixture::new();
    drop(open(&f));
    let named = unknown(&f);
    fs::write(&named, b"keep").unwrap();
    for value in [
        serde_json::json!({"version":1,"entries":[]}),
        serde_json::json!({"version":3,"entries":[]}),
        serde_json::json!({"version":2,"entries":[],"unexpected":true}),
        serde_json::json!({"version":2,"entries":[{"stage":named,"target":"untrusted","phase":"owned","fileIdentity":null}]}),
    ] {
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(document(&f), &bytes).unwrap();
        let mut registry = TransferRegistry::new(TransferLimits::default()).unwrap();
        assert_eq!(
            registry.initialize_journal(&f.metadata).unwrap_err().code,
            ErrorCode::StorageFailed
        );
        assert_eq!(fs::read(document(&f)).unwrap(), bytes);
        assert_eq!(fs::read(&named).unwrap(), b"keep");
    }
}
#[test]
fn a_rebound_diagnostic_parent_does_not_authorize_deleting_either_stage() {
    let f = Fixture::new();
    drop(open(&f));
    let original = f.root.join("original");
    fs::create_dir(&original).unwrap();
    let stage = original.join(format!(".shellfox-transfer-{}.partial", Uuid::new_v4()));
    fs::write(&stage, b"original").unwrap();
    fs::write(
        document(&f),
        serde_json::to_vec(&serde_json::json!({"version":2,"entries":[row(&stage)]})).unwrap(),
    )
    .unwrap();
    let moved = f.root.join("moved");
    fs::rename(&original, &moved).unwrap();
    fs::create_dir(&original).unwrap();
    fs::write(&stage, b"rebound").unwrap();
    let _registry = open(&f);
    assert_eq!(fs::read(&stage).unwrap(), b"rebound");
    assert_eq!(
        fs::read(moved.join(stage.file_name().unwrap())).unwrap(),
        b"original"
    );
}
#[test]
fn preserved_record_quota_fails_closed_without_new_stage_or_hidden_cleanup() {
    let f = Fixture::new();
    drop(open(&f));
    let paths = (0..32).map(|_| unknown(&f)).collect::<Vec<_>>();
    for path in &paths {
        fs::write(path, b"preserved").unwrap();
    }
    let bytes = serde_json::to_vec(
        &serde_json::json!({"version":2,"entries":paths.iter().map(|p|row(p)).collect::<Vec<_>>()}),
    )
    .unwrap();
    fs::write(document(&f), &bytes).unwrap();
    let mut registry = open(&f);
    assert_eq!(
        registry
            .begin_write(
                &super::streams::actor(),
                super::streams::capability(&f),
                &super::streams::create("new", 0),
                &super::streams::allow
            )
            .unwrap_err()
            .code,
        ErrorCode::StorageFailed
    );
    assert_eq!(fs::read_dir(&f.root).unwrap().count(), 32);
    for path in paths {
        assert_eq!(fs::read(path).unwrap(), b"preserved");
    }
    assert_eq!(fs::read(document(&f)).unwrap(), bytes);
}
