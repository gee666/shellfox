use crate::support::{loc, Fixture};
use shellfox_files::*;
use std::{fs, os::windows::io::AsRawHandle, ptr::null_mut};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{GetSecurityInfo, SetSecurityInfo, SE_FILE_OBJECT},
        GetSecurityDescriptorControl, DACL_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED,
    },
};

fn dacl(file: &fs::File, protect: bool) -> (Vec<u8>, bool) {
    let mut acl = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: output pointers are valid and descriptor owns the returned ACL.
    let code = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut acl,
            null_mut(),
            &mut descriptor,
        )
    };
    assert_eq!(code, 0);
    assert!(!acl.is_null());
    let mut control = 0;
    let mut revision = 0;
    // SAFETY: descriptor remains live, ACL carries its allocation length.
    let bytes = unsafe {
        assert_ne!(
            GetSecurityDescriptorControl(descriptor, &mut control, &mut revision),
            0
        );
        let bytes = std::slice::from_raw_parts(acl.cast::<u8>(), (*acl).AclSize as usize).to_vec();
        if protect {
            assert_eq!(
                SetSecurityInfo(
                    file.as_raw_handle(),
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
        LocalFree(descriptor);
        bytes
    };
    (bytes, control & SE_DACL_PROTECTED != 0)
}

#[test]
fn windows_atomic_save_keeps_protected_dacl_and_refuses_readonly() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new();
    f.write("private", "old");
    let original = fs::OpenOptions::new()
        .access_mode(0x20000 | 0x40000)
        .open(f.root.join("private"))
        .unwrap();
    dacl(&original, true);
    let before = dacl(&original, false);
    drop(original);
    let revision = f.service.read_text(&loc("private")).unwrap().revision;
    f.service
        .write_text(&WriteText {
            path: "private".into(),
            text: "new".into(),
            revision,
        })
        .unwrap();
    let current = fs::File::open(f.root.join("private")).unwrap();
    assert_eq!(dacl(&current, false), before);
    drop(current);
    let mut permissions = fs::metadata(f.root.join("private")).unwrap().permissions();
    let original_permissions = permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(f.root.join("private"), permissions.clone()).unwrap();
    let revision = f.service.read_text(&loc("private")).unwrap().revision;
    assert_eq!(
        f.service
            .write_text(&WriteText {
                path: "private".into(),
                text: "bad".into(),
                revision
            })
            .unwrap_err()
            .code,
        ErrorCode::AccessDenied
    );
    fs::set_permissions(f.root.join("private"), original_permissions).unwrap();
}
