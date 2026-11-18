use std::{
    fs,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
    ptr::null_mut,
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{GetSecurityInfo, SetSecurityInfo, SE_FILE_OBJECT},
        GetLengthSid, GetSecurityDescriptorControl, DACL_SECURITY_INFORMATION,
        OWNER_SECURITY_INFORMATION, UNPROTECTED_DACL_SECURITY_INFORMATION,
    },
};
fn open(path: &Path, write: bool) -> fs::File {
    fs::OpenOptions::new()
        .access_mode(0x20000 | if write { 0x40000 } else { 0 })
        .custom_flags(0x02000000)
        .open(path)
        .unwrap()
}
pub(super) fn security(path: &Path) -> Vec<u8> {
    let file = open(path, false);
    let mut owner = null_mut();
    let mut acl = null_mut();
    let mut descriptor = null_mut();
    let mut control = 0;
    let mut revision = 0;
    // SAFETY: live handle and output storage. Descriptor owns the valid SID and ACL.
    unsafe {
        assert_eq!(
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut acl,
                null_mut(),
                &mut descriptor
            ),
            0
        );
        assert_ne!(
            GetSecurityDescriptorControl(descriptor, &mut control, &mut revision),
            0
        );
        let mut bytes = control.to_le_bytes().to_vec();
        bytes.extend_from_slice(std::slice::from_raw_parts(
            owner.cast::<u8>(),
            GetLengthSid(owner) as usize,
        ));
        if !acl.is_null() {
            bytes.extend_from_slice(std::slice::from_raw_parts(
                acl.cast::<u8>(),
                (*acl).AclSize as usize,
            ));
        }
        LocalFree(descriptor);
        bytes
    }
}
pub(super) fn unprotect(path: &Path) {
    let file = open(path, true);
    let mut acl = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: fixture directory handle and output storage are live; descriptor owns ACL.
    unsafe {
        assert_eq!(
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                &mut acl,
                null_mut(),
                &mut descriptor
            ),
            0
        );
        let code = SetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | UNPROTECTED_DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            acl,
            null_mut(),
        );
        LocalFree(descriptor);
        assert_eq!(code, 0);
    }
}
