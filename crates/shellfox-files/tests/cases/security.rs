use crate::support::{loc, Fixture};
use shellfox_files::*;
use std::{fs, path::Path};

#[test]
fn approved_root_is_component_scoped_and_local_desktop_is_not_a_sandbox() {
    let f = Fixture::new();
    let other = Fixture::new();
    other.write("account.txt", "account-authorized");
    let absolute = other.root.join("account.txt").to_str().unwrap().to_owned();
    assert_eq!(
        f.service.read_text(&loc(&absolute)).unwrap_err().code,
        ErrorCode::AccessDenied
    );
    assert_eq!(f.service.capability_kind(), CapabilityKind::ApprovedRoot);
    let local = FileService::local_desktop(Access::ReadOnly);
    assert_eq!(local.capability_kind(), CapabilityKind::LocalDesktop);
    assert_eq!(
        local.read_text(&loc(&absolute)).unwrap().text,
        "account-authorized"
    );
    assert!(local.list(&loc(".")).is_err());
    assert_eq!(
        local
            .create_file(&CreateFile {
                path: absolute,
                text: "bad".into()
            })
            .unwrap_err()
            .code,
        ErrorCode::AccessDenied
    );
    for path in [
        "../outside",
        "a/../outside",
        "a\0b",
        "https://example.com/a",
        "//server/share",
        "\\\\server\\share",
        "C:relative",
        "a:stream",
    ] {
        assert!(
            f.service.read_text(&loc(path)).is_err(),
            "accepted {path:?}"
        );
    }
    let prefix = f.root.with_file_name(format!(
        "{}-suffix",
        f.root.file_name().unwrap().to_str().unwrap()
    ));
    assert_eq!(
        f.service
            .list(&loc(prefix.to_str().unwrap()))
            .unwrap_err()
            .code,
        ErrorCode::AccessDenied
    );
}

#[test]
fn read_only_and_revoked_capabilities_cannot_mutate() {
    let f = Fixture::new();
    f.write("a", "keep");
    let readonly = FileService::approved_root(&f.root, Access::ReadOnly).unwrap();
    assert_eq!(readonly.read_text(&loc("a")).unwrap().text, "keep");
    assert_eq!(
        readonly
            .delete(&Delete {
                path: "a".into(),
                kind: DeleteKind::File
            })
            .unwrap_err()
            .code,
        ErrorCode::AccessDenied
    );
    f.service.revoke().unwrap();
    assert_eq!(
        f.service.list(&loc(".")).unwrap_err().code,
        ErrorCode::AccessDenied
    );
    assert_eq!(
        f.service.create_directory(&loc("no")).unwrap_err().code,
        ErrorCode::AccessDenied
    );
    assert_eq!(f.read("a"), b"keep");
}

#[cfg(windows)]
use crate::support::junction as windows_junction;

fn directory_link(from: &Path, to: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(from, to).unwrap();
    #[cfg(windows)]
    windows_junction::create(from, to);
}

#[test]
fn links_and_reparse_ancestors_cannot_escape_or_be_replaced() {
    let f = Fixture::new();
    let outside = Fixture::new();
    outside.write("secret", "not-shared");
    directory_link(&outside.root, &f.root.join("link"));
    assert_eq!(
        f.service.stat(&loc("link")).unwrap().kind,
        EntryKind::Symlink
    );
    let list = f.service.list(&loc(".")).unwrap();
    assert_eq!(list.entries[0].kind, EntryKind::Symlink);
    for path in ["link", "link/secret"] {
        assert!(f.service.read_text(&loc(path)).is_err());
    }
    assert!(f.service.list(&loc("link")).is_err());
    assert!(f
        .service
        .create_file(&CreateFile {
            path: "link/new".into(),
            text: "bad".into()
        })
        .is_err());
    assert!(f
        .service
        .delete(&Delete {
            path: "link".into(),
            kind: DeleteKind::EmptyDirectory
        })
        .is_err());
    assert!(f
        .service
        .rename(&Rename {
            path: "link".into(),
            destination: "other".into()
        })
        .is_err());
    assert!(FileService::approved_root(f.root.join("link"), Access::ReadWrite).is_err());
    assert_eq!(outside.read("secret"), b"not-shared");
    assert!(!outside.root.join("new").exists());
}

#[cfg(unix)]
#[test]
fn replaced_root_is_never_silently_reauthorized() {
    let f = Fixture::new();
    let original = f
        .root
        .with_file_name(format!("rust-files-moved-{}", uuid::Uuid::new_v4()));
    fs::rename(&f.root, &original).unwrap();
    fs::create_dir(&f.root).unwrap();
    fs::write(f.root.join("new"), "other").unwrap();
    assert_eq!(
        f.service.list(&loc(".")).unwrap_err().code,
        ErrorCode::FileConflict
    );
    fs::remove_dir_all(&original).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_root_handle_prevents_root_replacement() {
    let f = Fixture::new();
    let moved = f
        .root
        .with_file_name(format!("rust-files-moved-{}", uuid::Uuid::new_v4()));
    assert!(fs::rename(&f.root, &moved).is_err());
    assert!(f.service.list(&loc(".")).is_ok());
}

#[cfg(unix)]
#[test]
fn save_preserves_permissions_and_refuses_special_files() {
    use std::{
        ffi::CString,
        os::unix::fs::{MetadataExt, PermissionsExt},
    };
    let f = Fixture::new();
    f.write("private", "old");
    fs::set_permissions(f.root.join("private"), fs::Permissions::from_mode(0o640)).unwrap();
    let revision = f.service.read_text(&loc("private")).unwrap().revision;
    f.service
        .write_text(&WriteText {
            path: "private".into(),
            text: "new".into(),
            revision,
        })
        .unwrap();
    assert_eq!(
        fs::metadata(f.root.join("private")).unwrap().mode() & 0o777,
        0o640
    );
    let fifo = CString::new(f.root.join("fifo").to_str().unwrap()).unwrap();
    // SAFETY: fifo is a valid terminated fixture path and mode has no unsupported bits.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    assert_eq!(
        f.service.read_text(&loc("fifo")).unwrap_err().code,
        ErrorCode::Unsupported
    );
    assert_eq!(
        f.service
            .delete(&Delete {
                path: "fifo".into(),
                kind: DeleteKind::File
            })
            .unwrap_err()
            .code,
        ErrorCode::Validation
    );
}
