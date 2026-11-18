use super::super::*;
use super::support::Fixture;
use shellfox_protocol::ErrorCode;
use std::fs;

#[test]
fn integration_file_aliases_are_errors_not_missing_installations() {
    let fixture = Fixture::new(SystemPlatform::Linux);
    let file = fixture.root.join("owned");
    let alias = fixture.root.join("alias");
    fs::write(&file, b"owned").unwrap();
    fs::hard_link(&file, &alias).unwrap();
    assert_eq!(
        fixture.files.read(&file).unwrap_err().code,
        ErrorCode::AuthFailed
    );
}

#[cfg(windows)]
#[test]
fn windows_staged_replacement_preserves_a_protected_owned_file_dacl() {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt};
    #[link(name = "advapi32")]
    extern "system" {
        fn GetFileSecurityW(
            file: *const u16,
            information: u32,
            descriptor: *mut c_void,
            size: u32,
            needed: *mut u32,
        ) -> i32;
        fn SetFileSecurityW(file: *const u16, information: u32, descriptor: *mut c_void) -> i32;
        fn SetSecurityDescriptorControl(descriptor: *mut c_void, mask: u16, bits: u16) -> i32;
    }
    fn descriptor(path: &[u16]) -> Vec<u32> {
        let mut needed = 0;
        // SAFETY: owned NUL-terminated fixture path; first call sizes the buffer.
        unsafe {
            GetFileSecurityW(path.as_ptr(), 4, std::ptr::null_mut(), 0, &mut needed);
        }
        assert!(needed > 0 && needed < 65536);
        let mut words = vec![0u32; (needed as usize).div_ceil(4)];
        assert_ne!(
            unsafe {
                GetFileSecurityW(
                    path.as_ptr(),
                    4,
                    words.as_mut_ptr().cast(),
                    (words.len() * 4) as u32,
                    &mut needed,
                )
            },
            0
        );
        words
    }
    let fixture = Fixture::new(SystemPlatform::Windows);
    let file = fixture.root.join("owned.xml");
    fs::write(&file, b"before").unwrap();
    let wide: Vec<u16> = file.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut security = descriptor(&wide);
    // SAFETY: only this owned temporary file is changed. ACEs are untouched;
    // protecting its DACL makes a lost inheritance/security flag observable.
    assert_ne!(
        unsafe { SetSecurityDescriptorControl(security.as_mut_ptr().cast(), 0x1000, 0x1000) },
        0
    );
    assert_ne!(
        unsafe { SetFileSecurityW(wide.as_ptr(), 0x80000004, security.as_mut_ptr().cast()) },
        0
    );
    let before_security = descriptor(&wide);
    let snapshot = fixture.files.read(&file).unwrap().unwrap();
    fixture
        .files
        .replace(&file, Some(&snapshot), Some(b"after"), false)
        .unwrap();
    let mut after_security = descriptor(&wide);
    let mut before_security = before_security;
    // ReplaceFile may mark the identical ACL as auto-inherited. This status
    // bit is not the DACL protection/inheritance policy. Preserve all ACE bytes
    // and all other control bits, particularly SE_DACL_PROTECTED.
    assert_ne!(after_security[0] & 0x1000_0000, 0);
    after_security[0] &= !0x0400_0000;
    before_security[0] &= !0x0400_0000;
    assert_eq!(after_security, before_security);
    assert_eq!(fs::read(file).unwrap(), b"after");
}
