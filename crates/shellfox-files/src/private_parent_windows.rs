use crate::{private_security, ErrorCode, FileError, Result};
use std::{fs::File, os::windows::io::AsRawHandle, ptr::null_mut};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{GetSecurityInfo, SE_FILE_OBJECT},
        EqualSid, GetAce, IsWellKnownSid, WinBuiltinAdministratorsSid, WinLocalSystemSid,
        ACCESS_ALLOWED_ACE, ACE_HEADER, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    },
};

// App roots are opened relative to the original pinned parent, and their own
// handles exclude delete sharing. Do not use this fallback for file creation.
pub(crate) fn verify_app_directory_parent(file: &File) -> Result<()> {
    verify(file).or_else(|error| {
        private_security::Security::new(true)?
            .require_owner(file)
            .map_err(|_| error)
    })
}

pub(crate) fn verify(file: &File) -> Result<()> {
    let current = private_security::current_sid()?;
    let mut owner = null_mut();
    let mut acl = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: all outputs are live; descriptor owns owner/ACL until LocalFree.
    let code = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut acl,
            null_mut(),
            &mut descriptor,
        )
    };
    if code != 0 {
        return Err(FileError::new(
            ErrorCode::AccessDenied,
            &format!("Private parent security query failed (Win32 {code})."),
        ));
    }
    let result = (|| {
        if owner.is_null() || acl.is_null() {
            return Err(FileError::new(
                ErrorCode::AccessDenied,
                "Private parent has missing owner or null DACL.",
            ));
        }
        let trusted = |sid| unsafe {
            EqualSid(sid, current.as_ptr().cast_mut().cast()) != 0
                || IsWellKnownSid(sid, WinLocalSystemSid) != 0
                || IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0
        };
        if !trusted(owner) {
            return Err(FileError::new(
                ErrorCode::AccessDenied,
                "Private parent owner is not the current user or a trusted OS principal.",
            ));
        }
        // Conservative effective-parent policy: no mutating allow to an untrusted SID.
        // Denies may reduce access; conditional/object-specific ACLs are unproven here.
        unsafe {
            if (*acl).AceCount > 1024 {
                return Err(FileError::denied());
            }
            for index in 0..(*acl).AceCount {
                let mut data = null_mut();
                if GetAce(acl, index as u32, &mut data) == 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
                let header = &*data.cast::<ACE_HEADER>();
                if header.AceFlags & 8 != 0 {
                    continue;
                } // inherit-only, not parent access
                if header.AceType == 1 {
                    continue;
                } // deny cannot grant mutation
                if header.AceType != 0
                    || (header.AceSize as usize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>()
                {
                    return Err(FileError::new(
                        ErrorCode::AccessDenied,
                        &format!(
                            "Private parent effective ACE type {} is unproven.",
                            header.AceType
                        ),
                    ));
                }
                let ace = &*data.cast::<ACCESS_ALLOWED_ACE>();
                const MUTATE: u32 = 0x2
                    | 0x4
                    | 0x10
                    | 0x40
                    | 0x100
                    | 0x10000
                    | 0x40000
                    | 0x80000
                    | 0x10000000
                    | 0x40000000;
                if ace.Mask & MUTATE != 0
                    && !trusted((&ace.SidStart as *const u32).cast_mut().cast())
                {
                    return Err(FileError::new(ErrorCode::AccessDenied,&format!("Private parent trust policy refuses an untrusted mutating allow ACE (mask 0x{:08x}).",ace.Mask)));
                }
            }
        }
        Ok(())
    })();
    unsafe {
        LocalFree(descriptor);
    }
    result
}
