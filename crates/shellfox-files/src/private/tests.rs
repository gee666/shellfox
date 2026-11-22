use super::*;
use crate::test_support::{link_dir, open_dir as handle, TempDir};
use std::fs;

fn name(value: &str) -> &OsStr {
    OsStr::new(value)
}
fn code(result: Result<impl Sized>) -> ErrorCode {
    match result {
        Ok(_) => panic!("expected an error"),
        Err(error) => error.code,
    }
}
fn entries(dir: &TempDir) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(&dir.path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn new_directories_and_files_are_private_from_the_first_handle() {
    let dir = TempDir::new();
    let parent = dir.handle();
    // A broad readable parent ACL must not reach the new objects.
    #[cfg(windows)]
    crate::test_support::grant_everyone(&parent, false);
    let created = create_dir(&parent, name("dir")).unwrap();
    native::require_private(created.as_file(), true).unwrap();
    created.verify().unwrap();
    let app = create_app_dir(&parent, name("app")).unwrap();
    native::require_private(app.as_file(), true).unwrap();
    let file = native::create_file(&parent, name("file")).unwrap();
    native::require_private(&file, false).unwrap();
    assert_eq!(file.metadata().unwrap().len(), 0);
}

#[test]
fn existing_entries_are_never_adopted_or_repaired() {
    let dir = TempDir::new();
    let parent = dir.handle();
    fs::create_dir(dir.join("plain")).unwrap();
    fs::write(dir.join("plain.json"), b"{}").unwrap();
    #[cfg(windows)]
    crate::test_support::grant_everyone(&handle(&dir.join("plain")), false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.join("plain"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(dir.join("plain.json"), fs::Permissions::from_mode(0o644)).unwrap();
    }
    assert_eq!(
        code(create_dir(&parent, name("plain"))),
        ErrorCode::FileConflict
    );
    assert_eq!(
        code(open_dir(&parent, name("plain"))),
        ErrorCode::AccessDenied
    );
    assert_eq!(
        code(read(&parent, name("plain.json"), 64)),
        ErrorCode::AccessDenied
    );
    let version = write(&parent, name("fresh.json"), b"1", None).unwrap();
    assert_eq!(
        code(write(&parent, name("plain.json"), b"2", Some(&version))),
        ErrorCode::AccessDenied
    );
    assert_eq!(fs::read(dir.join("plain.json")).unwrap(), b"{}");
    assert!(open_dir(&parent, name("plain")).is_err(), "never repaired");
}

#[test]
fn links_and_junctions_are_refused() {
    let dir = TempDir::new();
    let parent = dir.handle();
    let target = create_dir(&parent, name("target")).unwrap();
    write(target.as_file(), name("leaf"), b"secret", None).unwrap();
    link_dir(&dir.join("target"), &dir.join("link"));
    assert!(open_dir(&parent, name("link")).is_err());
    assert!(open_app_dir(&parent, name("link")).is_err());
    assert_eq!(
        code(create_dir(&parent, name("link"))),
        ErrorCode::FileConflict
    );
    assert!(read(&parent, name("link"), 64).is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.join("target/leaf"), dir.join("file-link")).unwrap();
        assert!(read(&parent, name("file-link"), 64).is_err());
    }
    for bad in ["..", ".", "a/b", "a\\b", "c:x", ""] {
        assert!(create_dir(&parent, name(bad)).is_err(), "{bad}");
        assert!(read(&parent, name(bad), 64).is_err(), "{bad}");
    }
}

#[test]
fn hardlinked_private_files_are_refused() {
    let dir = TempDir::new();
    let parent = dir.handle();
    write(&parent, name("state.json"), b"{}", None).unwrap();
    assert_eq!(read(&parent, name("state.json"), 64).unwrap().bytes, b"{}");
    fs::hard_link(dir.join("state.json"), dir.join("alias.json")).unwrap();
    assert_eq!(
        code(read(&parent, name("state.json"), 64)),
        ErrorCode::AccessDenied
    );
    assert_eq!(
        code(read(&parent, name("alias.json"), 64)),
        ErrorCode::AccessDenied
    );
}

#[test]
fn writes_and_removes_require_the_expected_version() {
    let dir = TempDir::new();
    let parent = dir.handle();
    let first = write(&parent, name("state.json"), b"one", None).unwrap();
    assert_eq!(
        code(write(&parent, name("state.json"), b"two", None)),
        ErrorCode::FileConflict
    );
    let second = write(&parent, name("state.json"), b"two", Some(&first)).unwrap();
    assert_ne!(first, second);
    assert_eq!(
        code(write(&parent, name("state.json"), b"three", Some(&first))),
        ErrorCode::FileConflict
    );
    let read = read(&parent, name("state.json"), 64).unwrap();
    assert_eq!(
        (read.bytes.as_slice(), &read.version),
        (&b"two"[..], &second)
    );
    assert_eq!(
        code(super::read(&parent, name("state.json"), 2)),
        ErrorCode::Unsupported
    );
    assert_eq!(
        code(remove(&parent, name("state.json"), &first)),
        ErrorCode::FileConflict
    );
    assert_eq!(
        entries(&dir),
        ["state.json"],
        "failed writes leave no temp file"
    );
    remove(&parent, name("state.json"), &second).unwrap();
    assert!(entries(&dir).is_empty());
    assert_eq!(
        code(super::read(&parent, name("state.json"), 64)),
        ErrorCode::NotFound
    );
}

#[test]
fn a_parent_writable_by_others_is_refused() {
    let dir = TempDir::new();
    let parent = dir.handle();
    #[cfg(windows)]
    crate::test_support::grant_everyone(&parent, true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dir.path, fs::Permissions::from_mode(0o777)).unwrap();
    }
    assert_eq!(
        code(create_dir(&parent, name("dir"))),
        ErrorCode::AccessDenied
    );
    assert_eq!(
        code(write(&parent, name("f"), b"x", None)),
        ErrorCode::AccessDenied
    );
    assert!(entries(&dir).is_empty());
}

#[test]
fn repair_tightens_only_the_app_directory() {
    let dir = TempDir::new();
    let parent = dir.handle();
    fs::create_dir(dir.join("data")).unwrap();
    let original = handle(&dir.join("data"));
    #[cfg(windows)]
    crate::test_support::grant_everyone(&original, false);
    assert!(open_app_dir(&parent, name("data")).is_err());
    let before = parent.metadata().unwrap().permissions();
    repair_app_dir(&parent, name("data"), &original).unwrap();
    open_app_dir(&parent, name("data"))
        .unwrap()
        .verify()
        .unwrap();
    assert_eq!(parent.metadata().unwrap().permissions(), before);
    fs::create_dir(dir.join("other")).unwrap();
    assert_eq!(
        code(repair_app_dir(
            &parent,
            name("data"),
            &handle(&dir.join("other"))
        )),
        ErrorCode::FileConflict
    );
}

#[cfg(windows)]
#[test]
fn a_held_private_directory_cannot_be_renamed() {
    let dir = TempDir::new();
    let created = create_dir(&dir.handle(), name("pinned")).unwrap();
    assert!(fs::rename(dir.join("pinned"), dir.join("moved")).is_err());
    drop(created);
    fs::rename(dir.join("pinned"), dir.join("moved")).unwrap();
}

#[cfg(unix)]
#[test]
fn verify_detects_a_replaced_name() {
    let dir = TempDir::new();
    let parent = dir.handle();
    let created = create_dir(&parent, name("dir")).unwrap();
    fs::rename(dir.join("dir"), dir.join("moved")).unwrap();
    create_dir(&parent, name("dir")).unwrap();
    assert_eq!(code(created.verify()), ErrorCode::FileConflict);
}
