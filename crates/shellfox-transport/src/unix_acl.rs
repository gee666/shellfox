use crate::HelperError;
use std::os::fd::AsFd;
/// Inspect held-descriptor ACL/inheritance. Reject all entries, including benign
/// deny-only ACLs, and unknown/denied queries. No chmod-based privacy inference.
pub fn verify_held_acl(file: &std::fs::File) -> Result<(), HelperError> {
    verify_acl(file)
}
pub(crate) fn verify_acl(file: &impl AsFd) -> Result<(), HelperError> {
    #[cfg(target_os = "linux")]
    for name in ["system.posix_acl_access", "system.posix_acl_default"] {
        match rustix::fs::fgetxattr_query(file, name) {
            Err(rustix::io::Errno::NODATA) => {}
            _ => return Err(HelperError::PermissionDenied),
        }
    }
    #[cfg(target_os = "macos")]
    {
        darwin_held_acl(file)?;
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = file;
        return Err(HelperError::PermissionDenied);
    }
    Ok(())
}
#[cfg(target_os = "macos")]
fn darwin_held_acl(file: &impl AsFd) -> Result<(), HelperError> {
    use std::os::fd::AsRawFd;
    #[repr(C)]
    struct Attributes {
        count: u16,
        reserved: u16,
        common: u32,
        volume: u32,
        directory: u32,
        file: u32,
        fork: u32,
    }
    extern "C" {
        fn fgetattrlist(
            fd: i32,
            attributes: *mut Attributes,
            buffer: *mut std::ffi::c_void,
            length: usize,
            options: u32,
        ) -> i32;
    }
    let mut attributes = Attributes {
        count: 5,
        reserved: 0,
        common: 0x80400000,
        volume: 0,
        directory: 0,
        file: 0,
        fork: 0,
    };
    let mut buffer = [0u8; 4096];
    // SAFETY: native attrlist layout, held original fd, writable bounded reply.
    // REPORT_FULLSIZE lets the parser reject oversized/unsupported security data.
    let result = unsafe {
        fgetattrlist(
            file.as_fd().as_raw_fd(),
            &mut attributes,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            4,
        )
    };
    if result != 0 || !crate::acl_policy::darwin_acl_absent(&buffer) {
        return Err(HelperError::PermissionDenied);
    }
    Ok(())
}
