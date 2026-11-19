use crate::HelperError;
use std::os::fd::AsFd;

/// Private resources must not have access or inheritance ACL entries.
pub fn verify_held_acl(file: &std::fs::File) -> Result<(), HelperError> {
    verify_acl(file)
}

#[cfg(target_os = "macos")]
pub fn verify_held_traversal_acl(file: &std::fs::File) -> Result<(), HelperError> {
    // A deny-only ACL on an ancestor (for example deny delete on the user's
    // home directory) cannot enlarge access. Never change an ancestor's ACL.
    crate::acl_policy::verify(file, true).map_err(|_| HelperError::PermissionDenied)
}

#[cfg(target_os = "macos")]
pub fn tighten_held_acl(file: &impl AsFd) -> Result<(), HelperError> {
    crate::acl_policy::clear_owned(file).map_err(|_| HelperError::PermissionDenied)
}

pub(crate) fn verify_acl(file: &impl AsFd) -> Result<(), HelperError> {
    #[cfg(target_os = "linux")]
    for name in ["system.posix_acl_access", "system.posix_acl_default"] {
        match rustix::fs::fgetxattr_query(file, name) {
            Err(rustix::io::Errno::NODATA) => (),
            _ => return Err(HelperError::PermissionDenied),
        }
    }
    #[cfg(target_os = "macos")]
    crate::acl_policy::verify(file, false).map_err(|_| HelperError::PermissionDenied)?;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = file;
        return Err(HelperError::PermissionDenied);
    }
    Ok(())
}
