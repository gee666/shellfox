use super::*;
#[cfg(windows)]
use crate::private_test_junction as junction;
#[cfg(windows)]
use crate::private_test_windows as windows;
use crate::{
    EditorBackend, EditorCancellation, Location, TransferLimits, TransferRegistry, WriteText,
};
use std::{fs, path::PathBuf};
use uuid::Uuid;
fn security(path: &Path) -> Vec<u8> {
    #[cfg(windows)]
    {
        windows::security(path)
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let info = fs::metadata(path).unwrap();
        [
            info.uid().to_le_bytes(),
            info.gid().to_le_bytes(),
            info.mode().to_le_bytes(),
        ]
        .concat()
    }
}

struct Fixture {
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .canonicalize()
            .unwrap();
        let base = base
            .to_str()
            .unwrap()
            .trim_start_matches("\\\\?\\")
            .to_owned();
        let path = Path::new(&base).join(format!("rust-journal-anchor-{}", Uuid::new_v4()));
        fs::create_dir_all(path.join("documents")).unwrap();
        let issuer = Arc::new(FileService::local_desktop(Access::ReadWrite));
        let plan = issuer
            .plan_directory(&Location {
                path: path.join("journal").to_str().unwrap().into(),
            })
            .unwrap();
        drop(plan.create_private_approved_root().unwrap());
        drop(plan);
        drop(issuer);
        fs::create_dir(path.join("replacement")).unwrap();
        fs::write(path.join("documents/a.txt"), "old").unwrap();
        Self { path }
    }
    fn documents(&self) -> Arc<FileService> {
        Arc::new(
            FileService::approved_root(self.path.join("documents"), Access::ReadWrite).unwrap(),
        )
    }
    fn journal(&self, access: Access) -> Arc<FileService> {
        Arc::new(FileService::approved_root(self.path.join("journal"), access).unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.path).unwrap();
    }
}
fn exercise(backend: &EditorBackend) {
    let session = backend.session();
    let file = session
        .read(
            &Location {
                path: "a.txt".into(),
            },
            &EditorCancellation::default(),
        )
        .unwrap();
    session
        .write(
            &WriteText {
                path: file.path,
                revision: file.revision,
                text: "changed".into(),
            },
            &EditorCancellation::default(),
        )
        .unwrap();
}

#[test]
fn anchored_constructor_rejects_readonly_full_account_and_local_journals_before_mutation() {
    let f = Fixture::new();
    let path = f.path.join("journal");
    let before = security(&path);
    for journal in [
        f.journal(Access::ReadOnly),
        Arc::new(FileService::local_desktop(Access::ReadWrite)),
        Arc::new(FileService::approved_full_account()),
    ] {
        assert!(EditorBackend::new_anchored(f.documents(), journal).is_err());
        assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
        assert_eq!(security(&path), before);
    }
}

#[test]
fn source_readonly_authority_is_not_upgraded_by_a_writable_journal() {
    let f = Fixture::new();
    let source =
        Arc::new(FileService::approved_root(f.path.join("documents"), Access::ReadOnly).unwrap());
    let backend =
        EditorBackend::new_anchored(source.clone(), f.journal(Access::ReadWrite)).unwrap();
    assert!(Arc::ptr_eq(&backend.capability(), &source));
    let session = backend.session();
    let file = session
        .read(
            &Location {
                path: "a.txt".into(),
            },
            &EditorCancellation::default(),
        )
        .unwrap();
    assert!(session
        .write(
            &WriteText {
                path: file.path,
                revision: file.revision,
                text: "denied".into()
            },
            &EditorCancellation::default()
        )
        .is_err());
    assert_eq!(fs::read(f.path.join("documents/a.txt")).unwrap(), b"old");
}

#[test]
fn exact_root_is_retained_even_if_its_diagnostic_label_resolves_to_another_directory() {
    for alias in [false, true] {
        let f = Fixture::new();
        let mut journal = f.journal(Access::ReadWrite);
        // Fault injection models a pathname rebind on Windows, where retained handles otherwise
        // deny renaming the root. Actual Unix rename/alias races are covered separately below.
        let label = if alias {
            let path = f.path.join("journal-alias");
            #[cfg(unix)]
            std::os::unix::fs::symlink(f.path.join("replacement"), &path).unwrap();
            #[cfg(windows)]
            junction::create(&f.path.join("replacement"), &path);
            path
        } else {
            f.path.join("replacement")
        };
        Arc::get_mut(&mut journal)
            .unwrap()
            .root
            .as_mut()
            .unwrap()
            .resolution = crate::service::RootResolution::PathBound { path: label };
        fs::write(f.path.join("replacement/sentinel"), "do not touch").unwrap();
        let before = security(&f.path.join("replacement"));
        let backend = EditorBackend::new_anchored(f.documents(), journal).unwrap();
        exercise(&backend);
        assert_eq!(
            fs::read(f.path.join("documents/a.txt")).unwrap(),
            b"changed"
        );
        assert!(f.path.join("journal/file-transfer-stages-v2.json").exists());
        assert_eq!(fs::read_dir(f.path.join("replacement")).unwrap().count(), 1);
        assert_eq!(
            fs::read(f.path.join("replacement/sentinel")).unwrap(),
            b"do not touch"
        );
        assert_eq!(security(&f.path.join("replacement")), before);
    }
}

#[cfg(windows)]
#[test]
fn losing_protected_dacl_status_fences_the_original_journal() {
    let f = Fixture::new();
    let backend = EditorBackend::new_anchored(f.documents(), f.journal(Access::ReadWrite)).unwrap();
    let before = fs::read(f.path.join("journal/file-transfer-stages-v2.json")).unwrap();
    windows::unprotect(&f.path.join("journal"));
    assert!(backend
        .session()
        .read(
            &Location {
                path: "a.txt".into()
            },
            &EditorCancellation::default()
        )
        .is_err());
    assert_eq!(
        fs::read(f.path.join("journal/file-transfer-stages-v2.json")).unwrap(),
        before
    );
}

#[test]
fn directory_plan_authority_ancestry_and_revocation_are_not_regranted_by_initialization() {
    let f = Fixture::new();
    let parent = Arc::new(FileService::approved_root(&f.path, Access::ReadWrite).unwrap());
    let plan = parent
        .plan_directory(&Location {
            path: "journal".into(),
        })
        .unwrap();
    let journal = plan.create_approved_root().unwrap();
    let backend = EditorBackend::new_anchored(f.documents(), journal.clone()).unwrap();
    exercise(&backend);
    let before = fs::read(f.path.join("journal/file-transfer-stages-v2.json")).unwrap();
    parent.revoke().unwrap();
    let session = backend.session();
    assert!(session
        .read(
            &Location {
                path: "a.txt".into()
            },
            &EditorCancellation::default()
        )
        .is_err());
    assert!(session
        .write(
            &WriteText {
                path: "a.txt".into(),
                revision: "0".repeat(64),
                text: "denied".into()
            },
            &EditorCancellation::default()
        )
        .is_err());
    assert!(EditorBackend::new_anchored(f.documents(), journal).is_err());
    assert_eq!(
        fs::read(f.path.join("journal/file-transfer-stages-v2.json")).unwrap(),
        before
    );
    assert_eq!(
        fs::read(f.path.join("documents/a.txt")).unwrap(),
        b"changed"
    );
}

#[test]
fn revoked_original_journal_and_duplicate_lease_do_not_mutate_or_replace_state() {
    let f = Fixture::new();
    let revoked = f.journal(Access::ReadWrite);
    revoked.revoke().unwrap();
    assert!(EditorBackend::new_anchored(f.documents(), revoked).is_err());
    assert_eq!(fs::read_dir(f.path.join("journal")).unwrap().count(), 0);
    let journal = f.journal(Access::ReadWrite);
    let backend = EditorBackend::new_anchored(f.documents(), journal.clone()).unwrap();
    let before = fs::read(f.path.join("journal/file-transfer-stages-v2.json")).unwrap();
    let mut other = TransferRegistry::new(TransferLimits::default()).unwrap();
    assert!(other.initialize_journal_anchored(journal).is_err());
    assert_eq!(
        fs::read(f.path.join("journal/file-transfer-stages-v2.json")).unwrap(),
        before
    );
    exercise(&backend);
}

#[test]
fn journal_compare_and_replace_does_not_overwrite_an_external_version() {
    let f = Fixture::new();
    let backend = EditorBackend::new_anchored(f.documents(), f.journal(Access::ReadWrite)).unwrap();
    let session = backend.session();
    let file = session
        .read(
            &Location {
                path: "a.txt".into(),
            },
            &EditorCancellation::default(),
        )
        .unwrap();
    fs::write(
        f.path.join("journal/file-transfer-stages-v2.json"),
        "external version",
    )
    .unwrap();
    assert!(session
        .write(
            &WriteText {
                path: file.path,
                revision: file.revision,
                text: "denied".into()
            },
            &EditorCancellation::default()
        )
        .is_err());
    assert_eq!(
        fs::read(f.path.join("journal/file-transfer-stages-v2.json")).unwrap(),
        b"external version"
    );
    assert_eq!(fs::read(f.path.join("documents/a.txt")).unwrap(), b"old");
    assert!(!f.path.join("journal/.file-transfer-stages.next").exists());
}

#[cfg(unix)]
#[test]
fn real_replaced_and_symlinked_journal_paths_cannot_redirect_the_retained_fd() {
    for alias in [false, true] {
        let f = Fixture::new();
        let journal = f.journal(Access::ReadWrite);
        fs::rename(f.path.join("journal"), f.path.join("original-held")).unwrap();
        if alias {
            std::os::unix::fs::symlink(f.path.join("replacement"), f.path.join("journal")).unwrap();
        } else {
            fs::create_dir(f.path.join("journal")).unwrap();
        }
        let backend = EditorBackend::new_anchored(f.documents(), journal).unwrap();
        exercise(&backend);
        assert!(f
            .path
            .join("original-held/file-transfer-stages-v2.json")
            .exists());
        assert_eq!(fs::read_dir(f.path.join("replacement")).unwrap().count(), 0);
        assert_eq!(fs::read_dir(f.path.join("journal")).unwrap().count(), 0);
    }
}

#[cfg(unix)]
#[test]
fn broadening_original_root_permissions_fails_the_private_guard() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let backend = EditorBackend::new_anchored(f.documents(), f.journal(Access::ReadWrite)).unwrap();
    let before = fs::read(f.path.join("journal/file-transfer-stages-v2.json")).unwrap();
    fs::set_permissions(f.path.join("journal"), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(backend
        .session()
        .read(
            &Location {
                path: "a.txt".into()
            },
            &EditorCancellation::default()
        )
        .is_err());
    assert_eq!(
        fs::read(f.path.join("journal/file-transfer-stages-v2.json")).unwrap(),
        before
    );
}
