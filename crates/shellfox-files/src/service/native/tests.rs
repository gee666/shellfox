use super::*;
use crate::test_support::{link_dir, open_dir, TempDir};
use std::sync::atomic::AtomicBool;

struct Fence(AtomicBool);
impl NativeCommitFence for Fence {
    fn check(&self) -> Result<()> {
        if self.0.load(Ordering::Acquire) {
            return Err(FileError::denied());
        }
        Ok(())
    }
}
fn scope(dir: &TempDir) -> (Arc<FileService>, Arc<Fence>) {
    let fence = Arc::new(Fence(AtomicBool::new(false)));
    let scope = FileService::private_scope(dir.handle(), fence.clone()).unwrap();
    (scope, fence)
}
fn leaf(value: &str) -> &OsStr {
    OsStr::new(value)
}

#[test]
fn scopes_need_a_private_directory_and_take_only_leaf_names() {
    let dir = TempDir::new();
    std::fs::create_dir(dir.join("plain")).unwrap();
    #[cfg(windows)]
    crate::test_support::grant_everyone(&open_dir(&dir.join("plain")), false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(dir.join("plain"), mode).unwrap();
    }
    let fence = Arc::new(Fence(AtomicBool::new(false)));
    assert!(FileService::private_scope(open_dir(&dir.join("plain")), fence).is_err());
    let (scope, _) = scope(&dir);
    assert!(
        scope.private_child(leaf("plain")).is_err(),
        "never repaired"
    );
    for bad in ["../x", "a/b", "..", ""] {
        assert!(scope.write_private(leaf(bad), b"x", None).is_err());
        assert!(scope.read_private(leaf(bad), 8).is_err());
    }
    let path = Location {
        path: dir.path.to_str().unwrap().into(),
    };
    assert!(
        scope.list(&path).is_err(),
        "absolute paths are not leaf selectors"
    );
}

#[test]
fn writes_stop_at_revocation_or_a_lost_fence_but_cleanup_still_works() {
    let dir = TempDir::new();
    let (scope, fence) = scope(&dir);
    let child = scope.private_child(leaf("remote-v1")).unwrap();
    let version = child.write_private(leaf("a.json"), b"1", None).unwrap();
    assert_eq!(child.read_private(leaf("a.json"), 8).unwrap().bytes, b"1");
    fence.0.store(true, Ordering::Release);
    assert!(child.write_private(leaf("b.json"), b"1", None).is_err());
    fence.0.store(false, Ordering::Release);
    scope.revoke().unwrap();
    assert!(
        child.read_private(leaf("a.json"), 8).is_err(),
        "child follows parent"
    );
    assert!(child
        .write_private(leaf("a.json"), b"2", Some(&version))
        .is_err());
    child.remove_private(leaf("a.json"), &version).unwrap();
    assert!(!dir.join("remote-v1/a.json").exists());
    assert!(!dir.join("remote-v1/b.json").exists());
}

#[test]
fn private_roots_create_missing_parts_and_tighten_only_the_leaf() {
    let dir = TempDir::new();
    let nested = dir.join("a").join("b");
    let root = FileService::private_root(&nested).unwrap();
    let created = Location {
        path: nested.join("new.txt").to_str().unwrap().into(),
    };
    root.create_file(&CreateFile {
        path: created.path.clone(),
        text: "x".into(),
    })
    .unwrap();
    let parent = open_dir(&dir.join("a"));
    crate::private::open_app_dir(&parent, leaf("b")).unwrap();
    drop(root);
    let plain = dir.join("plain");
    std::fs::create_dir(&plain).unwrap();
    #[cfg(windows)]
    crate::test_support::grant_everyone(&open_dir(&plain), false);
    FileService::private_root(&plain).unwrap();
    crate::private::open_app_dir(&dir.handle(), leaf("plain")).unwrap();
    link_dir(&dir.join("a"), &dir.join("link"));
    assert!(FileService::private_root(&dir.join("link").join("c")).is_err());
    assert!(FileService::private_root(&dir.join("link")).is_err());
    assert!(!dir.join("a").join("c").exists());
}

#[test]
fn private_roots_refuse_aliased_spellings_before_creating_anything() {
    let dir = TempDir::new();
    std::fs::create_dir(dir.join("host")).unwrap();
    #[cfg(windows)]
    {
        assert!(FileService::private_root(&dir.join("HOST")).is_err());
        assert!(FileService::private_root(&dir.join("HOST").join("ui")).is_err());
        assert!(FileService::private_root(&dir.join("UI~1")).is_err());
        assert!(FileService::check_private_root(&dir.join("HOST").join("x")).is_err());
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.join("host"), dir.join("alias")).unwrap();
        assert!(FileService::private_root(&dir.join("alias").join("ui")).is_err());
    }
    assert!(std::fs::read_dir(dir.join("host"))
        .unwrap()
        .next()
        .is_none());
    FileService::check_private_root(&dir.join("host").join("ui").join("x")).unwrap();
    link_dir(&dir.join("host"), &dir.join("junction"));
    assert!(FileService::check_private_root(&dir.join("junction").join("x")).is_err());
    FileService::private_root(&dir.join("host").join("ui")).unwrap();
}

#[cfg(windows)]
#[test]
fn private_root_accepts_forward_slashes_on_every_run() {
    let dir = TempDir::new();
    let path = std::path::PathBuf::from(dir.join("app").to_str().unwrap().replace('\\', "/"));
    FileService::check_private_root(&path).unwrap();
    drop(FileService::private_root(&path).unwrap());
    drop(FileService::private_root(&path).unwrap());
}
