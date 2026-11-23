//! A private control directory scope over a scratch folder. Not a Service/Go ownership proof.
use shellfox_files::*;
use shellfox_protocol::CancellationToken;
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
};
struct Fence(CancellationToken);
impl NativeCommitFence for Fence {
    fn check(&self) -> Result<()> {
        if self.0.is_cancelled() {
            Err(FileError {
                code: ErrorCode::AccessDenied,
                message: "Fixture ownership lost.".into(),
            })
        } else {
            Ok(())
        }
    }
}
pub struct Fixture {
    pub files: Arc<FileService>,
    pub lost: CancellationToken,
    _parent: Arc<FileService>,
    // Last: scopes and handles close before the scratch folder is removed.
    scratch: tempfile::TempDir,
}
impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}
impl Fixture {
    pub fn new() -> Self {
        // No `..` in the spelling: private roots refuse it, and Unix absolute() keeps it.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        let scratch = tempfile::Builder::new()
            .prefix("native-metadata-")
            .tempdir_in(root)
            .unwrap();
        let parent_path = scratch.path().join("private-parent");
        let parent = FileService::private_root(&parent_path).unwrap();
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            const BACKUP_SEMANTICS: u32 = 0x0200_0000;
            options.custom_flags(BACKUP_SEMANTICS);
        }
        let handle = options.open(&parent_path).unwrap();
        let control = private::create_dir(&handle, OsStr::new("control")).unwrap();
        let lost = CancellationToken::new();
        let files =
            FileService::private_scope(control.into_file(), Arc::new(Fence(lost.clone()))).unwrap();
        Self {
            files,
            lost,
            _parent: parent,
            scratch,
        }
    }
    pub fn control_path(&self) -> PathBuf {
        self.scratch.path().join("private-parent/control")
    }
    /// Pre-existing metadata for a test, written outside the code under test.
    pub fn seed(files: &Arc<FileService>, leaf: &str, bytes: &[u8]) {
        files.write_private(OsStr::new(leaf), bytes, None).unwrap();
    }
}
