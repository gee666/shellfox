use sha2::{Digest, Sha256};
use shellfox_transport::VerifiedHelper;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
pub struct Scratch(pub PathBuf);
impl Scratch {
    pub fn new() -> Self {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let root = manifest.parent().unwrap().parent().unwrap().join("tmp");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(format!(
            "transport-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub fn executable() -> VerifiedHelper {
    let path = PathBuf::from(env!("CARGO_BIN_EXE_shellfox-fixture-helper"));
    let digest: [u8; 32] = Sha256::digest(std::fs::read(&path).unwrap()).into();
    VerifiedHelper::new(path, digest).unwrap()
}
