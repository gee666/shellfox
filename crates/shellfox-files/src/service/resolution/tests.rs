use super::*;
use crate::test_support::TempDir;
use std::sync::atomic::AtomicBool;

struct Fence(AtomicBool);
impl NativeCommitFence for Fence {
    fn check(&self) -> Result<()> {
        if self.0.load(Ordering::Acquire) {
            Err(FileError::denied())
        } else {
            Ok(())
        }
    }
}

#[test]
fn private_scopes_take_relative_selectors_and_never_escape_their_handle() {
    let dir = TempDir::new();
    let fence = Arc::new(Fence(AtomicBool::new(false)));
    let root = FileService::private_scope(dir.handle(), fence).unwrap();
    assert_eq!(root.root_path(), None);
    assert_eq!(root.capability_kind(), CapabilityKind::ApprovedRoot);
    let result = root
        .create_file(&CreateFile {
            path: "state.json".into(),
            text: "{}".into(),
        })
        .unwrap();
    assert_eq!(result.path, "state.json");
    assert_eq!(std::fs::read(dir.join("state.json")).unwrap(), b"{}");
    let listed = root.list(&Location { path: ".".into() }).unwrap();
    assert_eq!((listed.path.as_str(), listed.parent_path), (".", None));
    for path in [
        "/outside",
        "\\outside",
        "C:outside",
        "C:/outside",
        "\\\\server\\share\\key",
        "//server/share/key",
        "\\\\?\\C:\\key",
        "..",
        "../outside",
        "a/../outside",
        "a\\outside",
    ] {
        assert!(root.base(path).is_err(), "selector {path:?}");
    }
    for unsupported in [
        root.resolve_directory(&Location { path: ".".into() }).err(),
        root.complete_directory(&Location { path: "".into() }).err(),
    ] {
        assert_eq!(unsupported.unwrap().code, ErrorCode::Unsupported);
    }
}

#[test]
fn child_scopes_keep_every_parent_fence_and_revocation() {
    let dir = TempDir::new();
    let fence = Arc::new(Fence(AtomicBool::new(false)));
    let parent = FileService::private_scope(dir.handle(), fence.clone()).unwrap();
    let child = parent.private_child(std::ffi::OsStr::new("child")).unwrap();
    let grandchild = child.private_child(std::ffi::OsStr::new("leaf")).unwrap();
    grandchild.authorized(true).unwrap();
    fence.0.store(true, Ordering::Release);
    assert!(grandchild.authorized(true).is_err());
    grandchild.authorized(false).unwrap();
    fence.0.store(false, Ordering::Release);
    child.revoke().unwrap();
    assert!(grandchild.authorized(false).is_err());
    parent.authorized(true).unwrap();
}
