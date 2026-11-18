use super::*;
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
struct Fixture {
    path: PathBuf,
    service: FileService,
}
impl Fixture {
    fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .canonicalize()
            .unwrap();
        let root = root.to_str().unwrap().trim_start_matches("\\\\?\\");
        let path = Path::new(root).join(format!("rust-files-stream-race-{}", Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self {
            service: FileService::approved_root(&path, Access::ReadWrite).unwrap(),
            path,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(std::mem::replace(
            &mut self.service,
            FileService::local_desktop(Access::ReadOnly),
        ));
        fs::remove_dir_all(&self.path).unwrap();
    }
}
fn create(size: u64) -> BeginWrite {
    BeginWrite {
        path: "final".into(),
        size,
        mode: WriteMode::Create,
        revision: None,
        content: TransferContent::File,
        mime_type: None,
    }
}
#[test]
fn approval_revoked_after_flush_is_checked_at_the_publication_boundary() {
    let f = Fixture::new();
    let approved = AtomicBool::new(true);
    let check = || {
        if approved.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(FileError::denied())
        }
    };
    let mut stage = StagedWrite::begin(&f.service, &create(0), 10, &check).unwrap();
    assert_eq!(
        stage
            .commit_with_hook(&f.service, 10, &check, || {
                approved.store(false, Ordering::Release);
            })
            .unwrap_err()
            .code,
        ErrorCode::AccessDenied
    );
    drop(stage);
    assert_eq!(fs::read_dir(&f.path).unwrap().count(), 0);
}
#[test]
fn final_stage_modification_and_destination_creation_are_never_published() {
    let f = Fixture::new();
    let check = || Ok(());
    let mut stage = StagedWrite::begin(&f.service, &create(4), 10, &check).unwrap();
    stage
        .write(
            &f.service,
            &WriteChunk {
                transfer_id: Uuid::nil(),
                offset: 0,
                data: b"ours".to_vec(),
            },
            &check,
        )
        .unwrap();
    let path = f.path.join(&stage.name);
    assert_eq!(
        stage
            .commit_with_hook(&f.service, 10, &check, || {
                fs::write(&path, "evil").unwrap();
            })
            .unwrap_err()
            .code,
        ErrorCode::FileConflict
    );
    drop(stage);
    let mut stage = StagedWrite::begin(&f.service, &create(0), 10, &check).unwrap();
    assert_eq!(
        stage
            .commit_with_hook(&f.service, 10, &check, || {
                fs::write(f.path.join("final"), "winner").unwrap();
            })
            .unwrap_err()
            .code,
        ErrorCode::FileConflict
    );
    drop(stage);
    assert_eq!(fs::read(f.path.join("final")).unwrap(), b"winner");
    assert_eq!(fs::read_dir(&f.path).unwrap().count(), 1);
}
#[test]
fn final_external_save_is_not_overwritten_after_stage_flush() {
    let f = Fixture::new();
    fs::write(f.path.join("final"), "old").unwrap();
    let mut input = create(3);
    input.mode = WriteMode::Replace;
    input.revision = Some(content::revision(b"old"));
    let check = || Ok(());
    let mut stage = StagedWrite::begin(&f.service, &input, 10, &check).unwrap();
    stage
        .write(
            &f.service,
            &WriteChunk {
                transfer_id: Uuid::nil(),
                offset: 0,
                data: b"new".to_vec(),
            },
            &check,
        )
        .unwrap();
    assert_eq!(
        stage
            .commit_with_hook(&f.service, 10, &check, || {
                fs::write(f.path.join("final"), "external").unwrap();
            })
            .unwrap_err()
            .code,
        ErrorCode::FileConflict
    );
    drop(stage);
    assert_eq!(fs::read(f.path.join("final")).unwrap(), b"external");
}
