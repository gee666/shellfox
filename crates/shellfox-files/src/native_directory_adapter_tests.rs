use super::*;
#[path = "cap_private_metadata_tests.rs"]
mod cap_metadata;
#[path = "native_journal_gate_tests.rs"]
mod journal_gates;
use crate::{
    create_private_directory_at, create_private_file_at, open_existing_private_at, Location,
    PrivateFileUse, PrivateKind, WriteText,
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
struct Fence {
    lost: Arc<AtomicBool>,
    _custody: Arc<()>,
}
impl NativeCommitFence for Fence {
    fn check(&self) -> Result<()> {
        if self.lost.load(Ordering::Acquire) {
            Err(FileError::denied())
        } else {
            Ok(())
        }
    }
}
struct Fixture {
    path: PathBuf,
    parent: Option<fs::File>,
}
impl Fixture {
    fn new() -> Self {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .canonicalize()
            .unwrap();
        let base = base.to_str().unwrap().trim_start_matches("\\\\?\\");
        let path = Path::new(base).join(format!("rust-native-adapter-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        let mut options = fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options
                .access_mode(0x120089 | 0x40000)
                .custom_flags(0x02000000 | 0x00200000)
                .share_mode(3);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC);
        }
        let parent = options.open(&path).unwrap();
        let dir = Dir::from_std_file(parent.try_clone().unwrap());
        crate::upload_private::protect_retained_directory(&dir).unwrap();
        Self {
            path,
            parent: Some(parent),
        }
    }
    fn parent(&self) -> &fs::File {
        self.parent.as_ref().unwrap()
    }
    fn prepared(&self) -> CreatedPrivate {
        create_private_directory_at(self.parent(), std::ffi::OsStr::new("prepared")).unwrap()
    }
    fn issuer(&self) -> Arc<FileService> {
        Arc::new(FileService::local_desktop(Access::ReadWrite))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.parent = None;
        fs::remove_dir_all(&self.path).unwrap();
    }
}
fn fence() -> (Arc<AtomicBool>, Arc<()>, Arc<dyn NativeCommitFence>) {
    let lost = Arc::new(AtomicBool::new(false));
    let custody = Arc::new(());
    (
        lost.clone(),
        custody.clone(),
        Arc::new(Fence {
            lost,
            _custody: custody,
        }),
    )
}
fn text(parent: &fs::File, name: &str, value: &str) {
    let file =
        create_private_file_at(parent, std::ffi::OsStr::new(name), PrivateFileUse::Stage).unwrap();
    let mut handle = file.as_file();
    handle.write_all(value.as_bytes()).unwrap();
}
#[test]
fn approved_native_issuer_is_relative_only_and_never_reopens_its_label() {
    let f = Fixture::new();
    let parent = f.prepared();
    text(parent.as_file(), "inside", "owned");
    let original = f.issuer();
    let (_lost, _keeper, guard) = fence();
    let issuer = original
        .native_child_issuer(PreparedPrivateDirectoryRef::Created(&parent), guard)
        .unwrap();
    let root = issuer.parent_root();
    assert_eq!(root.capability_kind(), CapabilityKind::ApprovedRoot);
    assert!(root.root_path().is_none());
    assert_eq!(root.diagnostic_label(), Some("native-prepared-parent"));
    let read = root
        .read_text(&Location {
            path: "inside".into(),
        })
        .unwrap();
    assert_eq!(read.text, "owned");
    assert_eq!(read.path, "inside");
    for selector in [
        "/inside",
        "\\inside",
        "C:\\inside",
        "C:inside",
        "../inside",
        "a/../inside",
        "//server/share",
    ] {
        assert!(root
            .read_text(&Location {
                path: selector.into()
            })
            .is_err());
    }
    assert!(root
        .plan_directory(&Location {
            path: "child".into()
        })
        .is_err());
    assert!(root
        .resolve_location(
            &Location {
                path: "inside".into()
            },
            &crate::FileContext {
                cwd: f.path.to_str().unwrap().into(),
                home: None
            }
        )
        .is_err());
}
#[test]
fn readonly_provenance_requires_explicit_authorized_rw_reopen_of_the_same_object() {
    let f = Fixture::new();
    let parent = f.prepared();
    let original = f.issuer();
    let (_lost, _keeper, guard) = fence();
    let readonly = open_existing_private_at(
        f.parent(),
        std::ffi::OsStr::new("prepared"),
        PrivateKind::Directory,
        PrivateAccess::ReadOnly,
    )
    .unwrap();
    assert_eq!(readonly.requested_access(), PrivateAccess::ReadOnly);
    assert!(original
        .native_child_issuer(
            PreparedPrivateDirectoryRef::Verified(&readonly),
            guard.clone()
        )
        .is_err());
    let reopened = original
        .reopen_native_directory_rw(&readonly, guard.clone())
        .unwrap();
    assert_eq!(reopened.identity(), readonly.identity());
    assert_eq!(reopened.requested_access(), PrivateAccess::ReadWrite);
    #[cfg(windows)]
    assert!(reopened.directory_pin_is_held());
    assert_eq!(parent.identity(), reopened.identity());
    assert!(original
        .native_child_issuer(
            PreparedPrivateDirectoryRef::Verified(&reopened),
            guard.clone()
        )
        .is_ok());
    let ro = Arc::new(FileService::local_desktop(Access::ReadOnly));
    assert!(ro.reopen_native_directory_rw(&readonly, guard).is_err());
}
#[test]
fn source_kinds_scope_mismatch_and_revocation_never_upgrade_authority() {
    let f = Fixture::new();
    let parent = f.prepared();
    let original = f.issuer();
    let (_lost, _keeper, guard) = fence();
    for bad in [
        Arc::new(FileService::approved_full_account()),
        Arc::new(FileService::local_desktop(Access::ReadOnly)),
        Arc::new(FileService::approved_root(&f.path, Access::ReadWrite).unwrap()),
    ] {
        assert!(bad
            .native_child_issuer(PreparedPrivateDirectoryRef::Created(&parent), guard.clone())
            .is_err());
    }
    let issuer = original
        .native_child_issuer(PreparedPrivateDirectoryRef::Created(&parent), guard.clone())
        .unwrap();
    let wrong =
        create_private_directory_at(f.parent(), std::ffi::OsStr::new("wrong-parent")).unwrap();
    assert!(issuer.created_directory_root(&wrong).is_err());
    let file = create_private_file_at(
        parent.as_file(),
        std::ffi::OsStr::new("file"),
        PrivateFileUse::Stage,
    )
    .unwrap();
    assert!(issuer.created_directory_root(&file).is_err());
    let child =
        create_private_directory_at(parent.as_file(), std::ffi::OsStr::new("child")).unwrap();
    text(child.as_file(), "value", "keep");
    let cap = issuer.created_directory_root(&child).unwrap();
    assert_eq!(
        cap.read_text(&Location {
            path: "value".into()
        })
        .unwrap()
        .text,
        "keep"
    );
    original.revoke().unwrap();
    assert!(cap
        .read_text(&Location {
            path: "value".into()
        })
        .is_err());
    assert!(issuer.created_directory_root(&child).is_err());
}
#[test]
fn later_fence_cannot_replace_or_weaken_an_inherited_native_restriction() {
    let f = Fixture::new();
    let parent = f.prepared();
    text(parent.as_file(), "inside", "keep");
    let original = f.issuer();
    let (lost, _keeper, guard) = fence();
    let issuer = original
        .native_child_issuer(PreparedPrivateDirectoryRef::Created(&parent), guard)
        .unwrap();
    let root = issuer.parent_root();
    let (_later_lost, _later_keeper, later) = fence();
    let derived = root
        .derive_anchored_relative(
            root.retained_anchored_directory().unwrap(),
            "additional-allowing-fence".into(),
            later,
        )
        .unwrap();
    let before = derived
        .read_text(&Location {
            path: "inside".into(),
        })
        .unwrap();
    lost.store(true, Ordering::Release);
    assert!(derived
        .write_text(&WriteText {
            path: "inside".into(),
            revision: before.revision,
            text: "denied".into()
        })
        .is_err());
}
#[test]
fn synchronous_fence_is_inherited_and_keeps_original_custody_without_watcher() {
    let f = Fixture::new();
    let parent = f.prepared();
    text(parent.as_file(), "inside", "keep");
    let original = f.issuer();
    let (lost, keeper, guard) = fence();
    let weak = Arc::downgrade(&keeper);
    let issuer = original
        .native_child_issuer(PreparedPrivateDirectoryRef::Created(&parent), guard.clone())
        .unwrap();
    let root = issuer.parent_root();
    let child =
        create_private_directory_at(parent.as_file(), std::ffi::OsStr::new("child")).unwrap();
    let cap = issuer.created_directory_root(&child).unwrap();
    drop(issuer);
    drop(guard);
    drop(keeper);
    assert!(weak.upgrade().is_some());
    let before = root
        .read_text(&Location {
            path: "inside".into(),
        })
        .unwrap();
    lost.store(true, Ordering::Release);
    // This is a synchronous MUTATION restriction, not a read/NoOwner proof.
    assert!(root
        .write_text(&WriteText {
            path: "inside".into(),
            revision: before.revision,
            text: "denied".into()
        })
        .is_err());
    assert!(cap
        .create_directory(&Location {
            path: "denied".into()
        })
        .is_err());
    assert_eq!(fs::read(f.path.join("prepared/inside")).unwrap(), b"keep");
    drop(root);
    assert!(weak.upgrade().is_some());
    drop(cap);
    assert!(weak.upgrade().is_none());
}
