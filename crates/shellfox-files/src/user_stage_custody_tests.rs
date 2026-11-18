use super::*;
use crate::{Access, TransferLimits, TransferRegistry};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
use uuid::Uuid;
struct Fixture {
    root: PathBuf,
    cap: Arc<FileService>,
    registry: Option<TransferRegistry>,
}
impl Fixture {
    fn new() -> Self {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .canonicalize()
            .unwrap();
        let root = PathBuf::from(base.to_str().unwrap().trim_start_matches("\\\\?\\"))
            .join(format!("user-stage-v2-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("user")).unwrap();
        fs::create_dir(root.join("outside")).unwrap();
        let cap =
            Arc::new(FileService::approved_root(root.join("user"), Access::ReadWrite).unwrap());
        let mut f = Self {
            root,
            cap,
            registry: None,
        };
        f.reopen();
        f
    }
    fn reopen(&mut self) {
        drop(self.registry.take());
        let mut registry = TransferRegistry::new(TransferLimits::default()).unwrap();
        registry
            .initialize_journal(self.root.join("metadata"))
            .unwrap();
        self.registry = Some(registry);
    }
    fn journal(&self) -> crate::journal::SharedJournal {
        self.registry
            .as_ref()
            .unwrap()
            .journal
            .as_ref()
            .unwrap()
            .clone()
    }
    fn reserve(&self) -> (Arc<UserStageCustody>, PathBuf) {
        let name = PathBuf::from(format!(".shellfox-transfer-{}.partial", Uuid::new_v4()));
        let target = self.cap.target("document.txt").unwrap();
        let mutation = FileService::mutation_lock().unwrap();
        let owner = self
            .journal()
            .lock()
            .unwrap()
            .reserve_stage(self.cap.clone(), "document.txt", &target, &name, &mutation)
            .unwrap();
        (owner, self.root.join("user").join(name))
    }
    fn finish(&self, owner: &UserStageCustody, stage: &Path) {
        let mutation = FileService::mutation_lock().unwrap();
        let removed = owner.dispose().unwrap();
        self.journal()
            .lock()
            .unwrap()
            .finish_stage(stage, removed, &mutation)
            .unwrap();
    }
    fn document(&self) -> PathBuf {
        self.root.join("metadata/file-transfer-stages-v2.json")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.registry.take());
        drop(std::mem::replace(
            &mut self.cap,
            Arc::new(FileService::local_desktop(Access::ReadOnly)),
        ));
        fs::remove_dir_all(&self.root).unwrap();
    }
}
#[test]
fn v2_postcreate_panic_keeps_same_original_fd_in_preinstalled_owner_without_poisoning_mutation() {
    let f = Fixture::new();
    let (owner, path) = f.reserve();
    let journal = f.journal();
    assert!(Arc::ptr_eq(
        &owner,
        journal
            .lock()
            .unwrap()
            .live
            .get(path.to_str().unwrap())
            .unwrap()
    ));
    let calls = AtomicUsize::new(0);
    let error = owner
        .create(&|| {
            if calls.fetch_add(1, Ordering::SeqCst) == if cfg!(windows) { 2 } else { 1 } {
                panic!("owned postcreate checkpoint");
            }
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, crate::ErrorCode::StorageFailed);
    {
        let capture = owner.capture.lock().unwrap();
        assert!(capture.file.is_some());
        assert!(capture
            .identity
            .as_ref()
            .unwrap()
            .matches(capture.file.as_ref().unwrap())
            .unwrap());
    }
    assert!(path.exists());
    f.finish(&owner, &path);
    assert!(!path.exists());
}
#[test]
fn v2_unknown_publication_disposition_preserves_bytes_and_restart_never_adopts_owner() {
    let mut f = Fixture::new();
    let (owner, path) = f.reserve();
    let mut file = owner.create(&|| Ok(())).unwrap();
    use std::io::Write;
    file.write_all(b"retain unknown draft").unwrap();
    file.sync_all().unwrap();
    drop(file);
    {
        let mutation = FileService::mutation_lock().unwrap();
        f.journal()
            .lock()
            .unwrap()
            .phase(&path, crate::journal::StagePhase::Publishing, &mutation)
            .unwrap();
    }
    owner.publishing().unwrap();
    f.finish(&owner, &path);
    drop(owner);
    assert_eq!(fs::read(&path).unwrap(), b"retain unknown draft");
    f.reopen();
    let journal = f.journal();
    let journal = journal.lock().unwrap();
    assert!(journal.live.is_empty());
    assert_eq!(journal.entries.len(), 1);
    assert!(matches!(
        journal.entries.values().next().unwrap().phase,
        crate::journal::StagePhase::PreservedUnknown
    ));
    assert_eq!(fs::read(path).unwrap(), b"retain unknown draft");
}
#[test]
fn v2_legacy_and_forged_stage_paths_are_diagnostics_not_cleanup_authority() {
    let mut f = Fixture::new();
    drop(f.registry.take());
    let unrelated = f
        .root
        .join("outside")
        .join(format!(".shellfox-transfer-{}.partial", Uuid::new_v4()));
    fs::write(&unrelated, b"unrelated outside scope").unwrap();
    let legacy = f.root.join("metadata/file-transfer-stages.json");
    let next = f.root.join("metadata/.file-transfer-stages.next");
    fs::write(&legacy, b"legacy unknown").unwrap();
    fs::write(&next, b"legacy next").unwrap();
    fs::write(f.document(),serde_json::to_vec(&serde_json::json!({"version":2,"entries":[{"stage":unrelated.to_str().unwrap(),"target":"outside scope","phase":"owned"}]})).unwrap()).unwrap();
    let before = fs::read(f.document()).unwrap();
    f.reopen();
    assert_eq!(fs::read(&unrelated).unwrap(), b"unrelated outside scope");
    assert_eq!(fs::read(&legacy).unwrap(), b"legacy unknown");
    assert_eq!(fs::read(&next).unwrap(), b"legacy next");
    assert_eq!(fs::read(f.document()).unwrap(), before);
    assert!(f.journal().lock().unwrap().live.is_empty());
}
#[test]
fn v2_metadata_cas_refuses_replacement_before_any_user_stage_creation() {
    let f = Fixture::new();
    fs::write(f.document(), b"external metadata change").unwrap();
    let name = PathBuf::from(format!(".shellfox-transfer-{}.partial", Uuid::new_v4()));
    let target = f.cap.target("document.txt").unwrap();
    let mutation = FileService::mutation_lock().unwrap();
    assert!(f
        .journal()
        .lock()
        .unwrap()
        .reserve_stage(f.cap.clone(), "document.txt", &target, &name, &mutation)
        .is_err());
    assert!(!f.root.join("user").join(name).exists());
    assert_eq!(fs::read(f.document()).unwrap(), b"external metadata change");
}
#[test]
fn v2_revocation_denies_forward_io_but_original_receipt_cleans_its_exact_stage() {
    let f = Fixture::new();
    let (owner, path) = f.reserve();
    drop(owner.create(&|| Ok(())).unwrap());
    f.cap.revoke().unwrap();
    assert!(owner.check().is_err());
    f.finish(&owner, &path);
    assert!(!path.exists());
}
#[cfg(windows)]
#[test]
fn v2_windows_creator_fd_pins_stage_name_and_cleanup_uses_that_original_delete_handle() {
    let f = Fixture::new();
    let (owner, path) = f.reserve();
    let working = owner.create(&|| Ok(())).unwrap();
    assert!(fs::rename(&path, f.root.join("outside/rebound")).is_err());
    assert!(fs::remove_file(&path).is_err());
    drop(working);
    f.finish(&owner, &path);
    assert!(!path.exists());
    assert!(!f.root.join("outside/rebound").exists());
}
#[cfg(unix)]
#[test]
fn v2_untrusted_user_stage_parent_is_denied_without_permission_repair_or_effect() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let user = f.root.join("user");
    fs::set_permissions(&user, fs::Permissions::from_mode(0o777)).unwrap();
    let before = fs::read(f.document()).unwrap();
    let target = f.cap.target("document.txt").unwrap();
    let name = PathBuf::from(format!(".shellfox-transfer-{}.partial", Uuid::new_v4()));
    let mutation = FileService::mutation_lock().unwrap();
    assert!(f
        .journal()
        .lock()
        .unwrap()
        .reserve_stage(f.cap.clone(), "document.txt", &target, &name, &mutation)
        .is_err());
    assert_eq!(
        fs::metadata(&user).unwrap().permissions().mode() & 0o777,
        0o777
    );
    assert_eq!(fs::read_dir(&user).unwrap().count(), 0);
    assert_eq!(fs::read(f.document()).unwrap(), before);
}
#[path = "user_write_consistency_tests.rs"]
mod consistency;
#[path = "user_stage_transfer_tests.rs"]
mod transfers;
#[cfg(windows)]
#[path = "user_stage_windows_probe_tests.rs"]
mod windows_probe;
#[test]
fn v2_existing_unprotected_metadata_root_is_not_repaired() {
    let f = Fixture::new();
    let path = f.root.join("unprotected");
    fs::create_dir(&path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let root = Arc::new(FileService::approved_root(&path, Access::ReadWrite).unwrap());
    assert!(crate::EditorBackend::new_anchored(f.cap.clone(), root).is_err());
    assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
}
