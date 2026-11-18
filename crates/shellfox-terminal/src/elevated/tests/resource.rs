use super::super::*;
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tmp");
        std::fs::create_dir_all(&root).unwrap();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::path::absolute(root.join(format!(
            "elevated-resource-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
        .unwrap();
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn helper_requires_an_explicit_trusted_hash_and_regular_absolute_image() {
    const IMAGE: &[u8] = b"owned helper fixture, not executable\n";
    let scratch = Scratch::new();
    let path = scratch.0.join("helper.exe");
    std::fs::write(&path, IMAGE).unwrap();
    // Trust derives from fixed test source bytes, not the file found on disk.
    let expected = TrustedHash::parse(&format!("{:x}", Sha256::digest(IMAGE))).unwrap();
    let helper = VerifiedHelper::open(&path, expected).unwrap();
    assert!(helper.path().is_absolute());
    assert_eq!(helper.path().file_name().unwrap(), "helper.exe");
    assert!(matches!(
        VerifiedHelper::open(&path, TrustedHash::parse(&"f".repeat(64)).unwrap()),
        Err(AdapterError::IntegrityMismatch)
    ));
    assert!(TrustedHash::parse(&"f".repeat(63)).is_err());
    assert!(TrustedHash::parse(&"g".repeat(64)).is_err());
    assert!(matches!(
        VerifiedHelper::open(std::path::Path::new("relative.exe"), expected),
        Err(AdapterError::InvalidResource)
    ));
    #[cfg(windows)]
    assert!(std::fs::write(&path, b"cannot replace retained image").is_err());
    drop(helper);
}

#[test]
fn resource_verification_never_follows_redirected_image() {
    let scratch = Scratch::new();
    let target = scratch.0.join("owned-target.exe");
    std::fs::write(&target, b"owned target").unwrap();
    let link = scratch.0.join("redirect.exe");
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&target, &link).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let expected = TrustedHash::parse(&format!("{:x}", Sha256::digest(b"owned target"))).unwrap();
    assert!(matches!(
        VerifiedHelper::open(&link, expected),
        Err(AdapterError::InvalidResource)
    ));
}
