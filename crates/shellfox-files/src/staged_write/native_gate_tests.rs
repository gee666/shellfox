//! No-mutation gate regressions on owned scratch. Not journal/recovery authority acceptance.
use super::*;
use cap_std::{ambient_authority, fs::Dir};
use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
struct AllowExisting;
impl NativeCommitFence for AllowExisting {
    fn check(&self) -> Result<()> {
        Ok(())
    }
}
struct Fixture {
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .canonicalize()
            .unwrap();
        let base = PathBuf::from(base.to_str().unwrap().trim_start_matches("\\\\?\\"));
        let path = base.join(format!("native-user-stage-gate-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        fs::create_dir(path.join("native")).unwrap();
        fs::create_dir(path.join("native/nested")).unwrap();
        Self { path }
    }
    fn native(&self) -> Arc<FileService> {
        let issuer = FileService::local_desktop(Access::ReadWrite);
        Arc::new(
            issuer
                .derive_anchored_relative(
                    Dir::open_ambient_dir(self.path.join("native"), ambient_authority()).unwrap(),
                    "not-a-path".into(),
                    Arc::new(AllowExisting),
                )
                .unwrap(),
        )
    }
    fn input(path: &str) -> BeginWrite {
        BeginWrite {
            path: path.into(),
            size: 0,
            mode: WriteMode::Create,
            revision: None,
            content: TransferContent::File,
            mime_type: None,
        }
    }
    fn names(&self) -> Vec<std::ffi::OsString> {
        let mut names = fs::read_dir(self.path.join("native"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        names.sort();
        names
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.path).unwrap();
    }
}
#[test]
fn native_user_stage_direct_gate_precedes_callbacks_and_any_journal_access() {
    let fixture = Fixture::new();
    let native = fixture.native();
    let journal = Arc::new(Mutex::new(
        crate::journal::StagingJournal::open(&fixture.path.join("journal")).unwrap(),
    ));
    let document = fixture.path.join("journal/file-transfer-stages-v2.json");
    let before = fs::read(&document).unwrap();
    let names = fixture.names();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _lock = journal.lock().unwrap();
        panic!("injected journal poison");
    }));
    for path in ["final", "nested/final"] {
        let error = StagedWrite::begin_with_journal(
            native.clone(),
            &Fixture::input(path),
            10,
            &|| panic!("gate must precede callback"),
            Some(journal.clone()),
        )
        .err()
        .unwrap();
        assert_eq!(error.code, ErrorCode::Unsupported);
        let error = StagedWrite::begin(&native, &Fixture::input(path), 10, &|| {
            panic!("no unjournaled fallback")
        })
        .err()
        .unwrap();
        assert_eq!(error.code, ErrorCode::Unsupported);
    }
    assert_eq!(fs::read(document).unwrap(), before);
    assert_eq!(fixture.names(), names);
    assert!(!fixture
        .path
        .join("journal/.file-transfer-stages.next")
        .exists());
    drop(journal);
    drop(native);
}
#[test]
fn native_user_stage_transfer_gate_precedes_expiry_cleanup_and_journal_rewrite() {
    let fixture = Fixture::new();
    let native = fixture.native();
    let path_bound = Arc::new(
        FileService::approved_root(fixture.path.join("native"), Access::ReadWrite).unwrap(),
    );
    let mut registry = TransferRegistry::new(TransferLimits {
        max_file_bytes: 10,
        idle_ttl: Duration::from_secs(1),
    })
    .unwrap();
    registry
        .initialize_journal(fixture.path.join("journal"))
        .unwrap();
    let actor = TransferActor::new("owned-gate-test".into(), Uuid::new_v4()).unwrap();
    let auth = |_: &TransferActor, cap: &FileService, write: bool| cap.authorized(write);
    let old = registry
        .begin_write(
            &actor,
            path_bound.clone(),
            &Fixture::input("retained"),
            &auth,
        )
        .unwrap();
    registry.entries.get_mut(&old.transfer_id).unwrap().touched =
        Instant::now() - Duration::from_secs(2);
    let before = fs::read(fixture.path.join("journal/file-transfer-stages-v2.json")).unwrap();
    let names = fixture.names();
    for path in ["final", "nested/final"] {
        assert_eq!(
            registry
                .begin_write(&actor, native.clone(), &Fixture::input(path), &auth)
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
    }
    assert!(registry.entries.contains_key(&old.transfer_id));
    assert_eq!(registry.usage().active, 1);
    assert_eq!(fixture.names(), names);
    assert_eq!(
        fs::read(fixture.path.join("journal/file-transfer-stages-v2.json")).unwrap(),
        before
    );
    assert!(!fixture
        .path
        .join("journal/.file-transfer-stages.next")
        .exists());
    drop(registry);
    drop(path_bound);
    drop(native);
}
