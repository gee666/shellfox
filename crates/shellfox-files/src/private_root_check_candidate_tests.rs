//! Guard/read-only candidate coverage. No publication owner or frontend/journal activation claim.
use super::*;
use crate::service::cap_private_metadata::current_private_candidate::{
    PrivateRootCheck, PrivateUse,
};
fn path_scope(f: &Fixture) -> Arc<FileService> {
    Arc::new(FileService::approved_root(f.path.join("prepared"), Access::ReadWrite).unwrap())
}
fn expose(path: &Path) {
    #[cfg(windows)]
    crate::private_test_windows::unprotect(path);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}
#[test]
fn pathbound_candidate_reads_only_original_private_leaf_and_proves_absence() {
    let f = Fixture::new();
    let parent = f.prepared();
    text(parent.as_file(), "value", "private");
    let scope = path_scope(&f);
    let check = PrivateRootCheck::verify(scope.clone()).unwrap();
    assert_eq!(
        check
            .read_optional_leaf(OsStr::new("value"), 64)
            .unwrap()
            .unwrap()
            .bytes,
        b"private"
    );
    assert!(check
        .read_optional_leaf(OsStr::new("absent"), 64)
        .unwrap()
        .is_none());
    assert!(check.read_optional_leaf(OsStr::new("absent"), 0).is_err());
    assert!(check
        .read_optional_leaf(OsStr::new("../outside"), 64)
        .is_err());
    let directory = create_private_directory_at(parent.as_file(), OsStr::new("directory")).unwrap();
    assert!(check
        .read_optional_leaf(OsStr::new("directory"), 64)
        .is_err());
    drop(directory);
    scope.revoke().unwrap();
    assert!(check.read_optional_leaf(OsStr::new("absent"), 64).is_err());
}
#[test]
fn current_root_privacy_is_separate_from_stable_identity_at_every_sensitive_boundary() {
    for boundary in [
        PrivateUse::Read,
        PrivateUse::Stage,
        PrivateUse::Write,
        PrivateUse::Commit,
    ] {
        let f = Fixture::new();
        let parent = f.prepared();
        text(parent.as_file(), "value", "private");
        let check = PrivateRootCheck::verify(path_scope(&f)).unwrap();
        check.check(boundary).unwrap();
        let identity = crate::HandleIdentity::from_file(parent.as_file()).unwrap();
        expose(&f.path.join("prepared"));
        assert_eq!(
            identity,
            crate::HandleIdentity::from_file(parent.as_file()).unwrap()
        );
        assert!(check.check(boundary).is_err());
        assert!(check.read_optional_leaf(OsStr::new("absent"), 64).is_err());
    }
}
#[test]
fn leaf_privacy_failure_is_not_missing_and_is_never_repaired() {
    let f = Fixture::new();
    let parent = f.prepared();
    text(parent.as_file(), "value", "private");
    let check = PrivateRootCheck::verify(path_scope(&f)).unwrap();
    expose(&f.path.join("prepared/value"));
    assert!(check.read_optional_leaf(OsStr::new("value"), 64).is_err());
    check.check(PrivateUse::Read).unwrap();
    #[cfg(windows)]
    assert!(!crate::private_test_windows::security(&f.path.join("prepared/value")).is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            fs::metadata(f.path.join("prepared/value")).unwrap().mode() & 0o7777,
            0o755
        );
    }
}
#[test]
fn candidate_keeps_independent_binding_and_readonly_never_becomes_rw() {
    let f = Fixture::new();
    let parent = f.prepared();
    text(parent.as_file(), "value", "private");
    let scope = path_scope(&f);
    let weak = Arc::downgrade(&scope);
    let mut keeper = scope.directory_preparation_owner();
    let check = PrivateRootCheck::verify(scope.clone()).unwrap();
    keeper.preserve_stale().unwrap();
    drop(keeper);
    drop(scope);
    drop(parent);
    assert!(weak.upgrade().is_some());
    assert_eq!(
        check
            .read_optional_leaf(OsStr::new("value"), 64)
            .unwrap()
            .unwrap()
            .bytes,
        b"private"
    );
    drop(check);
    assert!(weak.upgrade().is_none());
    let readonly =
        Arc::new(FileService::approved_root(f.path.join("prepared"), Access::ReadOnly).unwrap());
    let check = PrivateRootCheck::verify(readonly).unwrap();
    check.check(PrivateUse::Read).unwrap();
    for boundary in [PrivateUse::Stage, PrivateUse::Write, PrivateUse::Commit] {
        assert!(check.check(boundary).is_err());
    }
}
#[test]
fn path_alias_or_changed_binding_rejects_without_redirected_read() {
    let f = Fixture::new();
    let parent = f.prepared();
    text(parent.as_file(), "value", "private");
    let source = f.path.join("prepared");
    let alias = f.path.join("alias");
    #[cfg(windows)]
    crate::private_test_junction::create(&source, &alias);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&source, &alias).unwrap();
    if let Ok(scope) = FileService::approved_root(&alias, Access::ReadWrite) {
        assert!(PrivateRootCheck::verify(Arc::new(scope)).is_err());
    }
    let check = PrivateRootCheck::verify(path_scope(&f)).unwrap();
    match fs::rename(&source, f.path.join("moved")) {
        Ok(()) => {
            fs::create_dir(&source).unwrap();
            fs::write(source.join("value"), b"decoy").unwrap();
            assert!(check.read_optional_leaf(OsStr::new("value"), 64).is_err());
        }
        Err(_) => assert_eq!(
            check
                .read_optional_leaf(OsStr::new("value"), 64)
                .unwrap()
                .unwrap()
                .bytes,
            b"private"
        ),
    }
}
