use super::*;
use std::{
    fs,
    path::{Path, PathBuf},
};
#[path = "metadata_tests.rs"]
mod metadata_tests;
#[cfg(windows)]
use crate::private_test_windows as windows;
#[cfg(windows)]
#[path = "windows_tests.rs"]
mod windows_tests;

pub(super) struct Fixture {
    pub(super) path: PathBuf,
    parent: Option<File>,
}
impl Fixture {
    pub(super) fn new() -> Self {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tmp")
            .canonicalize()
            .unwrap();
        let base = base.to_str().unwrap().trim_start_matches("\\\\?\\");
        let path = Path::new(base).join(format!("rust-private-create-{}", uuid::Uuid::new_v4()));
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
            options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let parent = options.open(&path).unwrap();
        // Fixture preparation only: tests below inspect CHILD security at native create return.
        #[cfg(windows)]
        crate::private_security::Security::new(true)
            .unwrap()
            .apply(&parent)
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self {
            path,
            parent: Some(parent),
        }
    }
    pub(super) fn parent(&self) -> &File {
        self.parent.as_ref().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.parent = None;
        fs::remove_dir_all(&self.path).unwrap();
    }
}
fn state(path: &Path) -> Vec<u8> {
    #[cfg(windows)]
    {
        windows::security(path)
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = fs::metadata(path).unwrap();
        [
            metadata.uid().to_le_bytes(),
            metadata.gid().to_le_bytes(),
            metadata.mode().to_le_bytes(),
        ]
        .concat()
    }
}

#[test]
fn created_objects_are_private_on_first_native_handle_without_post_create_repair() {
    let f = Fixture::new();
    private_verify::parent(f.parent()).unwrap();
    for (name, kind) in [
        ("directory", PrivateKind::Directory),
        ("file", PrivateKind::File),
    ] {
        let file =
            private_native::create(f.parent(), OsStr::new(name), kind, PrivateFileUse::Stage)
                .unwrap();
        // Inspect immediately at native return, BEFORE the public wrapper could do any action.
        private_verify::object(&file, kind).unwrap();
        assert!(PrivateIdentity::from_file(&file)
            .unwrap()
            .matches(&file)
            .unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                file.metadata().unwrap().mode() & 0o7777,
                if kind == PrivateKind::Directory {
                    0o700
                } else {
                    0o600
                }
            );
        }
        drop(file);
    }
}
#[test]
fn exclusive_collision_and_existing_verification_never_repair_or_truncate() {
    let f = Fixture::new();
    fs::write(f.path.join("existing"), b"keep").unwrap();
    #[cfg(windows)]
    windows::unprotect(&f.path.join("existing"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(f.path.join("existing"), fs::Permissions::from_mode(0o644)).unwrap();
    }
    let before = state(&f.path.join("existing"));
    assert!(
        create_private_file_at(f.parent(), OsStr::new("existing"), PrivateFileUse::Stage).is_err()
    );
    assert!(open_existing_private_at(
        f.parent(),
        OsStr::new("existing"),
        PrivateKind::File,
        PrivateAccess::ReadOnly
    )
    .is_err());
    assert_eq!(fs::read(f.path.join("existing")).unwrap(), b"keep");
    assert_eq!(state(&f.path.join("existing")), before);
    for name in [
        "",
        ".",
        "..",
        "../escape",
        "/absolute",
        "a/b",
        "a\\b",
        "a:stream",
        "a\0b",
    ] {
        assert!(create_private_directory_at(f.parent(), OsStr::new(name)).is_err());
    }
    assert_eq!(fs::read_dir(&f.path).unwrap().count(), 1);
}
#[test]
fn sealed_created_and_verified_handles_keep_exact_binding_and_cleanup_only_owned_objects() {
    let f = Fixture::new();
    let mut created =
        create_private_file_at(f.parent(), OsStr::new("created"), PrivateFileUse::Stage).unwrap();
    assert!(!created.holds_exclusive_lease());
    created.verify_binding().unwrap();
    let existing = open_existing_private_at(
        f.parent(),
        OsStr::new("created"),
        PrivateKind::File,
        PrivateAccess::ReadOnly,
    )
    .unwrap();
    assert_eq!(created.identity(), existing.identity());
    drop(existing);
    fs::write(f.path.join("other"), b"other").unwrap();
    fs::rename(f.path.join("other"), f.path.join("created")).unwrap();
    assert!(created.remove_owned().is_err());
    assert_eq!(fs::read(f.path.join("created")).unwrap(), b"other");
    let mut directory = create_private_directory_at(f.parent(), OsStr::new("new-dir")).unwrap();
    // Directory link counts are NOT assumed to be one.
    directory.verify_binding().unwrap();
    directory.remove_owned().unwrap();
    assert_eq!(
        directory.cleanup_state(),
        PrivateCleanupState::DeletePending
    );
    drop(directory);
    assert!(!f.path.join("new-dir").exists());
}
#[test]
fn created_directory_handle_composes_with_a_second_nofollow_proof_open() {
    use cap_fs_ext::DirExt;
    let f = Fixture::new();
    let mut created =
        create_private_directory_at(f.parent(), OsStr::new("directory-proof")).unwrap();
    let parent = cap_std::fs::Dir::from_std_file(f.parent().try_clone().unwrap());
    let proof = parent.open_dir_nofollow("directory-proof").unwrap();
    assert!(created.identity().matches(&proof.into_std_file()).unwrap());
    created.remove_owned().unwrap();
}

#[test]
fn hardlinks_are_refused_for_files_and_lease_acquisition_is_not_a_purpose_flag() {
    let f = Fixture::new();
    let created =
        create_private_file_at(f.parent(), OsStr::new("linked"), PrivateFileUse::Stage).unwrap();
    fs::hard_link(f.path.join("linked"), f.path.join("alias")).unwrap();
    assert!(open_existing_private_at(
        f.parent(),
        OsStr::new("linked"),
        PrivateKind::File,
        PrivateAccess::ReadOnly
    )
    .is_err());
    drop(created);
    let mut lease = create_private_file_at(
        f.parent(),
        OsStr::new("lease"),
        PrivateFileUse::ExclusiveLease,
    )
    .unwrap();
    assert!(lease.holds_exclusive_lease());
    assert!(open_existing_private_at(
        f.parent(),
        OsStr::new("lease"),
        PrivateKind::File,
        PrivateAccess::ExclusiveLease
    )
    .is_err());
    assert!(lease.remove_owned().is_err()); // failure retains the acquired lease and custody
    assert!(lease.holds_exclusive_lease());
    drop(lease);
    assert!(f.path.join("lease").exists());
    let next = open_existing_private_at(
        f.parent(),
        OsStr::new("lease"),
        PrivateKind::File,
        PrivateAccess::ExclusiveLease,
    )
    .unwrap();
    assert!(next.holds_exclusive_lease());
}
#[cfg(unix)]
#[test]
fn lost_create_lock_race_never_unlinks_the_other_holders_acquired_lease() {
    let f = Fixture::new();
    let held = std::cell::RefCell::new(None);
    let result = retained::create_with(
        f.parent(),
        OsStr::new("race"),
        PrivateKind::File,
        PrivateFileUse::ExclusiveLease,
        |_| {
            let other = private_native::open(
                f.parent(),
                OsStr::new("race"),
                PrivateKind::File,
                PrivateAccess::ReadWrite,
            )
            .unwrap();
            private_native::lock(&other).unwrap();
            *held.borrow_mut() = Some(other);
            Ok(())
        },
    );
    assert!(result.is_err());
    assert!(f.path.join("race").exists());
    assert!(open_existing_private_at(
        f.parent(),
        OsStr::new("race"),
        PrivateKind::File,
        PrivateAccess::ExclusiveLease
    )
    .is_err());
    held.borrow_mut().take();
    assert!(open_existing_private_at(
        f.parent(),
        OsStr::new("race"),
        PrivateKind::File,
        PrivateAccess::ExclusiveLease
    )
    .is_ok());
}
#[cfg(unix)]
#[test]
fn original_parent_rename_does_not_redirect_creation_to_a_replacement() {
    let mut f = Fixture::new();
    let original = f.path.clone();
    let moved = f.path.with_extension("held");
    fs::rename(&original, &moved).unwrap();
    fs::create_dir(&original).unwrap();
    f.path = moved;
    let created = create_private_file_at(
        f.parent(),
        OsStr::new("only-original"),
        PrivateFileUse::Stage,
    )
    .unwrap();
    created.verify_binding().unwrap();
    assert_eq!(fs::read_dir(&original).unwrap().count(), 0);
    fs::remove_dir(&original).unwrap();
}
