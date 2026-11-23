//! Resolution/restriction mechanics through real private hooks, NOT native sealed-issuer,
//! ACL, acquired-token, ReFS or VFS acceptance.
use super::*;
use cap_std::ambient_authority;
use std::{fs, sync::atomic::AtomicBool};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .canonicalize()
            .unwrap();
        let base = PathBuf::from(base.to_str().unwrap().trim_start_matches("\\\\?\\"));
        let root = base.join(format!("anchored-resolution-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn dir(&self) -> Dir {
        Dir::open_ambient_dir(&self.0, ambient_authority()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
struct Restriction(AtomicBool);
impl NativeCommitFence for Restriction {
    fn check(&self) -> Result<()> {
        if self.0.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(FileError::denied())
        }
    }
}
fn allow() -> Arc<Restriction> {
    Arc::new(Restriction(AtomicBool::new(true)))
}
#[test]
fn anchored_relative_uses_held_directory_and_never_opens_its_label() {
    let fixture = Fixture::new();
    let issuer = FileService::local_desktop(Access::ReadWrite);
    let label = fixture
        .0
        .join("missing-label/not-authority")
        .to_str()
        .unwrap()
        .to_owned();
    let root = Arc::new(
        issuer
            .derive_anchored_relative(fixture.dir(), label.clone(), allow())
            .unwrap(),
    );
    assert!(root.is_anchored_relative());
    assert_eq!(root.root_path(), None);
    assert_eq!(root.capability_kind(), CapabilityKind::ApprovedRoot);
    assert_eq!(root.diagnostic_label(), Some(label.as_str()));
    let result = root
        .create_file(&CreateFile {
            path: "state.json".into(),
            text: "{}".into(),
        })
        .unwrap();
    assert_eq!(result.path, "state.json");
    assert_eq!(fs::read(fixture.0.join("state.json")).unwrap(), b"{}");
    assert!(!fixture.0.join("missing-label").exists());
    let listed = root.list(&Location { path: ".".into() }).unwrap();
    assert_eq!(listed.path, ".");
    assert_eq!(listed.parent_path, None);
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
        assert!(root.base(path).is_err(), "portable selector {path:?}");
    }
    assert_eq!(
        root.plan_directory(&Location {
            path: "child".into()
        })
        .err()
        .unwrap()
        .code,
        ErrorCode::Unsupported
    );
    assert_eq!(
        root.resolve_directory(&Location { path: ".".into() })
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
    assert_eq!(
        root.complete_directory(&Location { path: "".into() })
            .unwrap_err()
            .code,
        ErrorCode::Unsupported
    );
}
#[test]
fn later_fence_cannot_replace_old_restriction_or_upgrade_readonly_authority() {
    let fixture = Fixture::new();
    let first = allow();
    let later = allow();
    let issuer = FileService::local_desktop(Access::ReadWrite);
    let parent = issuer
        .derive_anchored_relative(fixture.dir(), "parent".into(), first.clone())
        .unwrap();
    let child = parent
        .derive_anchored_relative(fixture.dir(), "child".into(), later.clone())
        .unwrap();
    first.0.store(false, Ordering::Release);
    assert!(child.authorized(true).is_err());
    first.0.store(true, Ordering::Release);
    later.0.store(false, Ordering::Release);
    assert!(child.authorized(true).is_err());
    later.0.store(true, Ordering::Release);
    child.authorized(true).unwrap();
    let repeated = child
        .derive_anchored_relative(fixture.dir(), "same-restriction".into(), later)
        .unwrap();
    assert_eq!(repeated.native_commit_fences.len(), 2);
    assert!(repeated
        .derive_root(fixture.0.clone(), fixture.dir())
        .is_err());
    parent.revoke().unwrap();
    assert!(child.authorized(true).is_err());
    assert!(repeated.authorized(true).is_err());
    let readonly = FileService::local_desktop(Access::ReadOnly);
    assert!(readonly
        .derive_anchored_relative(fixture.dir(), "no-upgrade".into(), allow())
        .is_err());
}
