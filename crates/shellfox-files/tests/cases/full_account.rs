use crate::support::Fixture;
use shellfox_files::{
    Access, CapabilityKind, CreateFile, ErrorCode, FileService, Location, WriteText,
};
fn location(path: &std::path::Path) -> Location {
    Location {
        path: path.to_str().unwrap().into(),
    }
}
#[test]
fn legacy_root_validation_is_syntax_only_and_accepts_valid_dead_directories() {
    let fixture = Fixture::new();
    let dead = fixture.root.join("dead-old-approved-directory");
    assert!(!dead.exists());
    assert!(FileService::validate_legacy_root_syntax(&dead).is_ok());
    assert!(!dead.exists());
    for path in [
        "relative",
        "../parent",
        "https://example.invalid/root",
        "bad\0path",
    ] {
        assert!(FileService::validate_legacy_root_syntax(std::path::Path::new(path)).is_err());
    }
    #[cfg(windows)]
    for path in [
        r"C:relative",
        r"C:\\con",
        r"C:\\owned:ads",
        r"C:\\trailing.",
        r"\\\\?\\C:\\device",
        r"\\\\server\\share",
    ] {
        assert!(FileService::validate_legacy_root_syntax(std::path::Path::new(path)).is_err());
    }
}
#[test]
fn approved_account_is_explicit_and_reaches_owned_absolute_paths_without_root_enumeration() {
    let first = Fixture::new();
    let second = Fixture::new();
    first.write("one.txt", b"one");
    second.write("two.txt", b"two");
    let cap = FileService::approved_full_account();
    assert_eq!(cap.capability_kind(), CapabilityKind::ApprovedFullAccount);
    assert!(cap.root_path().is_none());
    assert_eq!(
        FileService::local_desktop(Access::ReadWrite).capability_kind(),
        CapabilityKind::LocalDesktop
    );
    assert_eq!(
        first.service.capability_kind(),
        CapabilityKind::ApprovedRoot
    );
    assert!(first
        .service
        .read_text(&location(&second.root.join("two.txt")))
        .is_err());
    assert!(
        cap.read_text(&location(&first.root.join("one.txt")))
            .unwrap()
            .text
            == "one"
    );
    assert!(
        cap.read_text(&location(&second.root.join("two.txt")))
            .unwrap()
            .text
            == "two"
    );
    assert_eq!(
        cap.read_text(&Location {
            path: "one.txt".into()
        })
        .unwrap_err()
        .code,
        ErrorCode::Validation
    );
}
#[test]
fn approved_account_preserves_cas_validation_os_errors_and_revocable_commits() {
    let fixture = Fixture::new();
    fixture.write("owned.txt", b"before");
    let path = fixture.root.join("owned.txt");
    let cap = FileService::approved_full_account();
    let initial = cap.read_text(&location(&path)).unwrap();
    cap.write_text(&WriteText {
        path: initial.path.clone(),
        text: "after".into(),
        revision: initial.revision.clone(),
    })
    .unwrap();
    assert_eq!(
        cap.write_text(&WriteText {
            path: initial.path,
            text: "stale".into(),
            revision: initial.revision
        })
        .unwrap_err()
        .code,
        ErrorCode::FileConflict
    );
    assert!(fixture.read("owned.txt") == b"after");
    assert_eq!(
        cap.read_text(&location(&fixture.root.join("missing")))
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(
        cap.read_text(&Location {
            path: format!("{}\0bad", path.display())
        })
        .unwrap_err()
        .code,
        ErrorCode::Validation
    );
    cap.revoke().unwrap();
    assert_eq!(
        cap.read_text(&location(&path)).unwrap_err().code,
        ErrorCode::AccessDenied
    );
    let created = fixture.root.join("not-created.txt");
    assert_eq!(
        cap.create_file(&CreateFile {
            path: created.to_str().unwrap().into(),
            text: "denied".into()
        })
        .unwrap_err()
        .code,
        ErrorCode::AccessDenied
    );
    assert!(!created.exists());
}
