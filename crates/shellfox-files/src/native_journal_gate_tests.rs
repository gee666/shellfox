use super::*;
use crate::{EditorBackend, TransferLimits, TransferRegistry, UploadInbox, UploadLimits};
use cap_fs_ext::MetadataExt;
fn native(f: &Fixture, parent: &CreatedPrivate) -> Arc<FileService> {
    let (_lost, _keeper, guard) = fence();
    f.issuer()
        .native_child_issuer(PreparedPrivateDirectoryRef::Created(parent), guard)
        .unwrap()
        .parent_root()
}
#[test]
fn native_journal_gate_precedes_lease_next_recovery_and_editor_initialization() {
    let f = Fixture::new();
    let parent = f.prepared();
    let user = create_private_directory_at(f.parent(), std::ffi::OsStr::new("user")).unwrap();
    let name = format!(".shellfox-transfer-{}.partial", uuid::Uuid::new_v4());
    text(user.as_file(), &name, "");
    let user_path = f.path.join("user");
    let stage = user_path.join(&name);
    let metadata = Dir::from_std_file(user.as_file().try_clone().unwrap())
        .dir_metadata()
        .unwrap();
    let document = serde_json::json!({"version":1,"entries":[{"stage":stage.to_str().unwrap(),"parent":user_path.to_str().unwrap(),"parentIdentity":{"dev":metadata.dev(),"ino":metadata.ino()},"fileIdentity":null}]});
    // Historical V1 bytes are deliberately NOT interpreted as V2 authority.
    let bytes = serde_json::to_string(&document).unwrap();
    text(parent.as_file(), "file-transfer-stages.json", &bytes);
    text(parent.as_file(), ".file-transfer-stages.next", "leave-next");
    let cap = native(&f, &parent);
    let mut registry = TransferRegistry::new(TransferLimits::default()).unwrap();
    let error = registry
        .initialize_journal_anchored(cap.clone())
        .err()
        .unwrap();
    assert_eq!(error.code, crate::ErrorCode::Unsupported);
    assert!(!registry.initialized);
    assert!(registry.journal.is_none());
    assert!(EditorBackend::new_anchored(f.issuer(), cap.clone()).is_err());
    let absent = f.path.join("must-not-create-editor-journal");
    assert!(EditorBackend::new(cap.clone(), &absent).is_err());
    assert!(!absent.exists());
    let ordinary =
        Arc::new(FileService::approved_root(f.path.join("prepared"), Access::ReadWrite).unwrap());
    assert!(EditorBackend::new_anchored(cap, ordinary).is_err());
    assert_eq!(
        fs::read(f.path.join("prepared/file-transfer-stages.json")).unwrap(),
        bytes.as_bytes()
    );
    assert_eq!(
        fs::read(f.path.join("prepared/.file-transfer-stages.next")).unwrap(),
        b"leave-next"
    );
    assert!(!f.path.join("prepared/.file-transfer-stages.lock").exists());
    assert!(stage.exists());
}
#[test]
fn v2_initialization_preserves_legacy_journal_and_unknown_next_without_recovery() {
    let f = Fixture::new();
    let journal = crate::journal::StagingJournal::open(&f.path.join("journal")).unwrap();
    drop(journal);
    let path = f.path.join("journal/file-transfer-stages.json");
    fs::write(&path, b"unknown historical journal").unwrap();
    let next = f.path.join("journal/.file-transfer-stages.next");
    fs::write(&next, b"preserve").unwrap();
    let journal = crate::journal::StagingJournal::open(&f.path.join("journal")).unwrap();
    assert!(journal.entries.is_empty());
    assert_eq!(fs::read(path).unwrap(), b"unknown historical journal");
    assert_eq!(fs::read(next).unwrap(), b"preserve");
}
#[test]
fn native_inbox_recovery_rejects_before_open_or_cleanup_of_legacy_root() {
    let f = Fixture::new();
    let parent = f.prepared();
    let mut inbox = UploadInbox::open(f.path.join("inbox"), UploadLimits::default()).unwrap();
    let dir = Dir::from_std_file(f.parent().try_clone().unwrap())
        .open_dir("inbox")
        .unwrap()
        .into_std_file();
    let id = uuid::Uuid::new_v4().to_string();
    let child = create_private_directory_at(&dir, std::ffi::OsStr::new(&id)).unwrap();
    text(child.as_file(), ".partial", "keep");
    inbox.capability = native(&f, &parent);
    assert_eq!(
        inbox.restore().err().unwrap().code,
        crate::ErrorCode::Unsupported
    );
    assert!(f.path.join("inbox").join(id).join(".partial").exists());
    assert!(inbox.saved.is_empty());
    inbox.dispose().unwrap();
}
