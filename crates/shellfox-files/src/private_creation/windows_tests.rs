use super::*;
use std::{os::windows::io::AsRawHandle, ptr::null_mut};
use windows_sys::Win32::Security::{
    AddAccessAllowedAceEx,
    Authorization::{SetSecurityInfo, SE_FILE_OBJECT},
    CreateWellKnownSid, InitializeAcl, WinWorldSid, ACL, DACL_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION,
};
fn parent_acl(parent: &File, other_write: bool) {
    let owner = crate::private_security::current_sid().unwrap();
    let mut world = [0usize; 9];
    let mut length = std::mem::size_of_val(&world) as u32;
    let mut storage = [0usize; 64];
    let acl = storage.as_mut_ptr().cast::<ACL>();
    // SAFETY: aligned bounded buffers own both SIDs/ACL through the completed fixture update.
    unsafe {
        assert_ne!(
            CreateWellKnownSid(
                WinWorldSid,
                null_mut(),
                world.as_mut_ptr().cast(),
                &mut length
            ),
            0
        );
        assert_ne!(
            InitializeAcl(acl, std::mem::size_of_val(&storage) as u32, 2),
            0
        );
        assert_ne!(
            AddAccessAllowedAceEx(acl, 2, 3, 0x1f01ff, owner.as_ptr().cast_mut().cast()),
            0
        );
        assert_ne!(
            AddAccessAllowedAceEx(
                acl,
                2,
                3,
                0x120089 | if other_write { 2 } else { 0 },
                world.as_mut_ptr().cast()
            ),
            0
        );
        assert_eq!(
            SetSecurityInfo(
                parent.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                acl,
                null_mut()
            ),
            0
        );
    }
}
#[test]
fn explicit_creation_descriptor_does_not_inherit_broad_readable_parent_acl() {
    let f = Fixture::new();
    parent_acl(f.parent(), false);
    private_verify::parent(f.parent()).unwrap();
    for (name, kind) in [
        ("private-dir", PrivateKind::Directory),
        ("private-file", PrivateKind::File),
    ] {
        let file =
            private_native::create(f.parent(), OsStr::new(name), kind, PrivateFileUse::Stage)
                .unwrap();
        // First native result, without ANY repair: exactly protected owner-only ACL must exist.
        private_verify::object(&file, kind).unwrap();
        if kind == PrivateKind::File {
            assert_eq!(file.metadata().unwrap().len(), 0);
        }
    }
}
#[test]
fn app_directory_repair_tightens_inherited_acl_without_touching_parent() {
    use cap_fs_ext::DirExt;
    let f = Fixture::new();
    parent_acl(f.parent(), true);
    let before = state(&f.path);
    let parent = cap_std::fs::Dir::from_std_file(f.parent().try_clone().unwrap());
    parent.create_dir("app-data").unwrap();
    parent.create_dir("other").unwrap();
    let name = OsStr::new("app-data");
    let held = parent.open_dir_nofollow(name).unwrap().into_std_file();
    let other = state(&f.path.join("other"));
    assert!(private_verify::object(&held, PrivateKind::Directory).is_err());
    assert!(repair_app_directory_at(f.parent(), OsStr::new("other"), &held).is_err());
    repair_app_directory_at(f.parent(), name, &held).unwrap();
    open_app_directory_at(f.parent(), name)
        .unwrap()
        .verify_binding()
        .unwrap();
    assert_eq!(state(&f.path.join("other")), other);
    assert_eq!(state(&f.path), before);
    assert!(create_private_directory_at(f.parent(), OsStr::new("still-denied")).is_err());
    create_app_directory_at(f.parent(), OsStr::new("app-created"))
        .unwrap()
        .verify_binding()
        .unwrap();
}

#[test]
fn readonly_native_parent_handle_can_create_when_original_logical_scope_is_rw() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new();
    let parent = fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x02000000 | 0x00200000)
        .share_mode(3)
        .open(&f.path)
        .unwrap();
    // The caller still owns logical RW authority; directory FD read access is not a new grant.
    let value = create_private_directory_at(&parent, OsStr::new("native-read-parent")).unwrap();
    assert!(value.directory_pin_is_held());
    value.verify_binding().unwrap();
}
#[test]
fn directory_pin_denies_rename_until_every_proof_handle_is_released() {
    use cap_fs_ext::DirExt;
    let f = Fixture::new();
    let value = create_private_directory_at(f.parent(), OsStr::new("pinned")).unwrap();
    assert!(value.directory_pin_is_held());
    let parent = cap_std::fs::Dir::from_std_file(f.parent().try_clone().unwrap());
    let proof = parent.open_dir_nofollow("pinned").unwrap();
    assert!(fs::rename(f.path.join("pinned"), f.path.join("moved")).is_err());
    drop(value.into_file());
    assert!(fs::rename(f.path.join("pinned"), f.path.join("moved")).is_err());
    drop(proof);
    fs::rename(f.path.join("pinned"), f.path.join("moved")).unwrap();
}
#[test]
fn failed_directory_cleanup_retains_parent_full_identity_and_released_pin_state() {
    use cap_fs_ext::DirExt;
    let f = Fixture::new();
    let mut value = create_private_directory_at(f.parent(), OsStr::new("pinned-cleanup")).unwrap();
    let id = value.identity().clone();
    let parent = cap_std::fs::Dir::from_std_file(f.parent().try_clone().unwrap());
    let proof = parent.open_dir_nofollow("pinned-cleanup").unwrap();
    assert!(value.remove_owned().is_err());
    assert_eq!(
        value.cleanup_state(),
        PrivateCleanupState::DirectoryPinReleased
    );
    assert!(!value.directory_pin_is_held());
    assert!(id.matches(value.as_file()).unwrap());
    assert!(fs::rename(f.path.join("pinned-cleanup"), f.path.join("rebound")).is_err());
    drop(proof);
    value.remove_owned().unwrap();
    assert_eq!(value.cleanup_state(), PrivateCleanupState::DeletePending);
    drop(value);
    assert!(!f.path.join("pinned-cleanup").exists());
}
#[test]
fn untrusted_parent_namespace_mutation_rights_are_rejected_before_create() {
    let f = Fixture::new();
    parent_acl(f.parent(), true);
    let before = state(&f.path);
    assert!(create_private_directory_at(f.parent(), OsStr::new("denied-dir")).is_err());
    assert!(
        create_private_file_at(f.parent(), OsStr::new("denied-file"), PrivateFileUse::Stage)
            .is_err()
    );
    assert_eq!(fs::read_dir(&f.path).unwrap().count(), 0);
    assert_eq!(state(&f.path), before);
}
