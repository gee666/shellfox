use shellfox_host::{Host, STORE_FILENAME};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .join(format!(
                "storage-guard-{}-{stamp}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[cfg(unix)]
fn link_file(source: &Path, target: &Path) -> bool {
    std::os::unix::fs::symlink(source, target).unwrap();
    true
}
#[cfg(windows)]
fn link_file(source: &Path, target: &Path) -> bool {
    match std::os::windows::fs::symlink_file(source, target) {
        Ok(()) => true,
        Err(e) if e.raw_os_error() == Some(1314) => false,
        Err(e) => panic!("symlink: {e}"),
    }
}

#[test]
fn existing_hardlinks_and_symlinks_to_outside_artifacts_are_rejected() {
    let scratch = Scratch::new();
    let outside = scratch.0.join("outside");
    drop(Host::open(&outside).unwrap());
    let source = outside.join(STORE_FILENAME);
    let original = fs::read(&source).unwrap();
    for artifact in [
        STORE_FILENAME.to_owned(),
        "shellfox-rust-v1.lock".to_owned(),
        format!("{STORE_FILENAME}-wal"),
        format!("{STORE_FILENAME}-shm"),
        format!("{STORE_FILENAME}-journal"),
    ] {
        for symlink in [false, true] {
            let root = scratch
                .0
                .join(format!("case-{}-{symlink}", artifact.replace('.', "_")));
            fs::create_dir(&root).unwrap();
            let target = root.join(&artifact);
            if symlink {
                if !link_file(&source, &target) {
                    eprintln!("Windows symlink privilege unavailable");
                    continue;
                }
            } else {
                fs::hard_link(&source, &target).unwrap();
            }
            assert!(Host::open(&root).is_err(), "accepted {artifact}");
            assert_eq!(
                fs::read(&source).unwrap(),
                original,
                "outside database changed"
            );
            if artifact != "shellfox-rust-v1.lock" {
                assert!(
                    !root.join("shellfox-rust-v1.lock").exists(),
                    "preflight must reject before lock or database open"
                );
            }
        }
    }
}

#[cfg(unix)]
fn link_dir(source: &Path, target: &Path) -> bool {
    std::os::unix::fs::symlink(source, target).unwrap();
    true
}
#[cfg(windows)]
fn link_dir(source: &Path, target: &Path) -> bool {
    match std::os::windows::fs::symlink_dir(source, target) {
        Ok(()) => true,
        Err(e) if e.raw_os_error() == Some(1314) => false,
        Err(e) => panic!("symlink directory: {e}"),
    }
}

#[test]
fn root_directory_links_are_rejected_before_creating_children() {
    let scratch = Scratch::new();
    let outside = scratch.0.join("outside-dir");
    fs::create_dir(&outside).unwrap();
    let link = scratch.0.join("linked-dir");
    if link_dir(&outside, &link) {
        assert!(Host::open(link.join("new-store")).is_err());
        assert!(!outside.join("new-store").exists());
    } else {
        eprintln!("Windows symlink privilege unavailable");
    }
    let nested = scratch.0.join("new").join("nested");
    drop(Host::open(&nested).unwrap());
    assert!(nested.join(STORE_FILENAME).exists());
}

#[test]
fn invalid_root_and_artifact_types_are_rejected() {
    let scratch = Scratch::new();
    let root = scratch.0.join("not-a-directory");
    fs::write(&root, b"unchanged").unwrap();
    assert!(Host::open(&root).is_err());
    assert_eq!(fs::read(&root).unwrap(), b"unchanged");
    let root = scratch.0.join("store");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join(format!("{STORE_FILENAME}-journal"))).unwrap();
    assert!(Host::open(&root).is_err());
    assert!(!root.join(STORE_FILENAME).exists());
    fs::remove_dir(root.join(format!("{STORE_FILENAME}-journal"))).unwrap();
    if link_file(&scratch.0.join("missing"), &root.join(STORE_FILENAME)) {
        assert!(Host::open(&root).is_err());
        assert!(!root.join("shellfox-rust-v1.lock").exists());
    }
}

#[test]
fn legacy_links_and_sidecars_are_rejected_before_import() {
    let scratch = Scratch::new();
    let outside = scratch.0.join("outside");
    drop(Host::open(&outside).unwrap());
    let source = outside.join(STORE_FILENAME);
    let original = fs::read(&source).unwrap();
    let host = Host::open(scratch.0.join("host")).unwrap();
    let root = scratch.0.join("legacy");
    fs::create_dir(&root).unwrap();
    let linked = root.join("legacy.sqlite3");
    fs::hard_link(&source, &linked).unwrap();
    assert!(host.import_legacy(&linked).is_err());
    fs::remove_file(&linked).unwrap();
    if link_file(&source, &linked) {
        assert!(host.import_legacy(&linked).is_err());
        fs::remove_file(&linked).unwrap();
    }
    fs::copy(&source, &linked).unwrap();
    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = root.join(format!("legacy.sqlite3{suffix}"));
        fs::hard_link(&source, &sidecar).unwrap();
        assert!(host.import_legacy(&linked).is_err());
        fs::remove_file(&sidecar).unwrap();
        if link_file(&source, &sidecar) {
            assert!(host.import_legacy(&linked).is_err());
            fs::remove_file(&sidecar).unwrap();
        }
    }
    assert_eq!(fs::read(source).unwrap(), original);
    assert_eq!(host.snapshot().unwrap().revision, 0);
}
