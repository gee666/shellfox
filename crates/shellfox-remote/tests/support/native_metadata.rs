//! Actual owned filesystem/capability fixture, NOT a Service/Go ownership or native-launch proof.
use shellfox_files::*;
use shellfox_protocol::CancellationToken;
use std::{
    ffi::OsStr,
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
};
struct Fence(CancellationToken);
impl NativeCommitFence for Fence {
    fn check(&self) -> Result<()> {
        if self.0.is_cancelled() {
            Err(FileError {
                code: ErrorCode::AccessDenied,
                message: "Owned fixture fence lost.".into(),
            })
        } else {
            Ok(())
        }
    }
}
pub struct Fixture {
    pub files: Arc<FileService>,
    pub lost: CancellationToken,
    _created: CreatedPrivate,
    _parent: File,
    _plan: DirectoryPlan,
    _scope: Arc<FileService>,
    keeper: DirectoryPreparationOwner,
    // Last: this fixture's files/scopes/pins drop before owned scratch cleanup.
    scratch: Option<tempfile::TempDir>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = self.keeper.preserve_stale() {
            if let Some(scratch) = self.scratch.take() {
                eprintln!(
                    "Fixture directory disposition unconfirmed: {error}; evidence retained at {}",
                    scratch.keep().display()
                );
            }
        }
    }
}
impl Fixture {
    pub fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        std::fs::create_dir_all(&root).unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        #[cfg(windows)]
        let root = PathBuf::from(
            root.to_str()
                .unwrap()
                .strip_prefix(r"\\?\")
                .unwrap_or(root.to_str().unwrap()),
        );
        let scratch = tempfile::Builder::new()
            .prefix("native-metadata-")
            .tempdir_in(root)
            .unwrap();
        let approved =
            Arc::new(FileService::approved_root(scratch.path(), Access::ReadWrite).unwrap());
        let keeper = approved.directory_preparation_owner();
        let plan = approved
            .plan_directory(&Location {
                path: "private-parent".into(),
            })
            .unwrap();
        let scope = plan.create_private_approved_root().unwrap();
        plan.revalidate().unwrap();
        keeper.verify_retained().unwrap();
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            const DIRECTORY_BACKUP_SEMANTICS: u32 = 0x02000000;
            options.custom_flags(DIRECTORY_BACKUP_SEMANTICS);
        }
        let parent = options.open(plan.configured_path()).unwrap();
        plan.revalidate().unwrap();
        // This unchanged primitive checks the ACTUAL new parent's strict policy.
        let created = create_private_directory_at(&parent, OsStr::new("control")).unwrap();
        created.verify_binding().unwrap();
        plan.revalidate().unwrap();
        let lost = CancellationToken::new();
        let issuer = Arc::new(FileService::local_desktop(Access::ReadWrite));
        let native = issuer
            .native_child_issuer(
                PreparedPrivateDirectoryRef::Created(&created),
                Arc::new(Fence(lost.clone())),
            )
            .unwrap();
        let files = native.parent_root();
        Self {
            files,
            lost,
            _created: created,
            _parent: parent,
            _plan: plan,
            _scope: scope,
            keeper,
            scratch: Some(scratch),
        }
    }
    pub fn control_path(&self) -> PathBuf {
        self.scratch
            .as_ref()
            .expect("fixture scratch retained")
            .path()
            .join("private-parent/control")
    }
    pub fn seed(files: &Arc<FileService>, leaf: &str, bytes: &[u8]) {
        // Fixture preparation of known pre-existing metadata, not the application approval write protocol.
        let mut owner = files
            .prepare_private_leaf_publication(
                OsStr::new(leaf),
                OsStr::new(&format!("seed-{}", uuid::Uuid::new_v4().simple())),
                None,
            )
            .unwrap();
        owner.write_all(bytes).unwrap();
        assert!(matches!(
            owner.publish(),
            PrivatePublishOutcome::Published(_)
        ));
        owner.preserve_stale();
    }
}
