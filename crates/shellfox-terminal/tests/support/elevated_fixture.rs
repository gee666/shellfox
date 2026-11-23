use sha2::{Digest, Sha256};
use shellfox_terminal::elevated::{Launch, PreparedLaunch, TrustedHash, VerifiedHelper};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Fixture {
    pub root: PathBuf,
    pub image: PathBuf,
    pub hash: TrustedHash,
}
impl Fixture {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        std::fs::create_dir_all(&root).unwrap();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::path::absolute(root.join(format!(
            "elevated-stdio-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
        .unwrap();
        std::fs::create_dir(&root).unwrap();
        // Cargo built this exact owned fixture source. This is a trusted test
        // build receipt, not trust derived from an arbitrary discovered helper.
        let source = PathBuf::from(env!("CARGO_BIN_EXE_shellfox-elevated-test-helper"));
        let bytes = std::fs::read(&source).unwrap();
        let hash = TrustedHash::parse(&format!("{:x}", Sha256::digest(&bytes))).unwrap();
        let image = root.join("Shellfox.ElevatedTerminal.exe");
        std::fs::copy(source, &image).unwrap();
        Self { root, image, hash }
    }
    pub fn verified(&self) -> VerifiedHelper {
        VerifiedHelper::open(&self.image, self.hash).unwrap()
    }
    pub fn launch(&self) -> PreparedLaunch {
        Launch::fixture(self.root.clone(), BTreeMap::new(), 80, 24)
            .unwrap()
            .prepare()
            .unwrap()
    }
}
/// Waits for fixture processes that outlived their launcher (see
/// `hand_to_host` in elevated_helper.rs): their image and cwd are in `root`.
#[cfg(windows)]
fn wait_for_handed_processes(root: &std::path::Path) {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, WAIT_OBJECT_0},
        System::Threading::WaitForSingleObject,
    };
    let Ok(list) = std::fs::read_to_string(root.join("owned-processes")) else {
        return;
    };
    for line in list.lines() {
        let handle = line.parse::<usize>().unwrap() as windows_sys::Win32::Foundation::HANDLE;
        // SAFETY: the fixture duplicated this handle into this process for us alone.
        let result = unsafe {
            let result = WaitForSingleObject(handle, 15_000);
            CloseHandle(handle);
            result
        };
        if result != WAIT_OBJECT_0 && !std::thread::panicking() {
            panic!("fixture process in {} did not exit", root.display());
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        #[cfg(windows)]
        wait_for_handed_processes(&self.root);
        if let Err(error) = std::fs::remove_dir_all(&self.root) {
            if std::thread::panicking() {
                eprintln!("owned fixture retained at {}: {error}", self.root.display());
            } else {
                panic!("remove owned fixture {}: {error}", self.root.display());
            }
        }
    }
}
