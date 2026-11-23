//! Windows security descriptors: the owner-only DACL used at creation, the exact check for
//! private objects, and the parent-trust rule.
use crate::{ErrorCode, FileError, Result};
use std::{fs::File, os::windows::io::AsRawHandle, ptr::null_mut};
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree},
    Security::{
        AddAccessAllowedAceEx,
        Authorization::{GetSecurityInfo, SetSecurityInfo, SE_FILE_OBJECT},
        CreateWellKnownSid, EqualSid, GetAce, GetLengthSid, GetSecurityDescriptorControl,
        GetTokenInformation, InitializeAcl, InitializeSecurityDescriptor, IsWellKnownSid,
        SetSecurityDescriptorControl, SetSecurityDescriptorDacl, SetSecurityDescriptorOwner,
        TokenUser, WinBuiltinAdministratorsSid, WinLocalSystemSid, ACCESS_ALLOWED_ACE, ACE_HEADER,
        ACL, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION, SECURITY_DESCRIPTOR, SE_DACL_PROTECTED, TOKEN_USER,
    },
};

#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(
        process: *mut std::ffi::c_void,
        access: u32,
        token: *mut *mut std::ffi::c_void,
    ) -> i32;
}
pub(crate) fn current_sid() -> Result<Vec<u8>> {
    struct Token(*mut std::ffi::c_void);
    impl Drop for Token {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    let mut handle = null_mut();
    // SAFETY: -1 is the current-process pseudo handle; output points to live storage.
    if unsafe { OpenProcessToken(-1isize as *mut _, 8, &mut handle) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let token = Token(handle);
    let mut size = 0;
    // SAFETY: this first call queries output length only.
    unsafe {
        GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut size);
    }
    if size == 0 || size > 64 * 1024 {
        return Err(FileError::denied());
    }
    let mut data = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    // SAFETY: aligned buffer covers size bytes and owns the returned SID through the copy below.
    unsafe {
        if GetTokenInformation(
            token.0,
            TokenUser,
            data.as_mut_ptr().cast(),
            size,
            &mut size,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let sid = (*data.as_ptr().cast::<TOKEN_USER>()).User.Sid;
        let length = GetLengthSid(sid) as usize;
        if length == 0 || length > 68 {
            return Err(FileError::denied());
        }
        Ok(std::slice::from_raw_parts(sid.cast::<u8>(), length).to_vec())
    }
}

pub(crate) struct Security {
    sid: Vec<u8>,
    acl: Vec<usize>,
    descriptor: SECURITY_DESCRIPTOR,
}
impl Security {
    pub fn new(directory: bool) -> Result<Self> {
        Self::descriptor(directory, false)
    }
    pub fn app_directory() -> Result<Self> {
        Self::descriptor(true, true)
    }
    fn descriptor(directory: bool, with_system: bool) -> Result<Self> {
        let sid = current_sid()?;
        let mut system = [0u32; 17];
        let mut system_size = std::mem::size_of_val(&system) as u32;
        // SAFETY: buffer and length describe writable, aligned SID storage.
        if with_system
            && unsafe {
                CreateWellKnownSid(
                    WinLocalSystemSid,
                    null_mut(),
                    system.as_mut_ptr().cast(),
                    &mut system_size,
                )
            } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let length = 16
            + sid.len()
            + if with_system {
                8 + system_size as usize
            } else {
                0
            };
        let mut acl = vec![0usize; length.div_ceil(std::mem::size_of::<usize>())];
        let mut descriptor = std::mem::MaybeUninit::<SECURITY_DESCRIPTOR>::uninit();
        // SAFETY: buffers are aligned/live and remain allocated for the descriptor's lifetime.
        unsafe {
            let acl_ptr = acl.as_mut_ptr().cast::<ACL>();
            if InitializeAcl(acl_ptr, length as u32, 2) == 0
                || AddAccessAllowedAceEx(
                    acl_ptr,
                    2,
                    if directory { 3 } else { 0 },
                    0x1f01ff,
                    sid.as_ptr().cast_mut().cast(),
                ) == 0
                // Do not change the ACL policy of files created inside this directory.
                || (with_system && EqualSid(sid.as_ptr().cast_mut().cast(), system.as_mut_ptr().cast()) == 0
                    && AddAccessAllowedAceEx(acl_ptr, 2, 0, 0x1f01ff, system.as_mut_ptr().cast()) == 0)
                || InitializeSecurityDescriptor(descriptor.as_mut_ptr().cast(), 1) == 0
                || SetSecurityDescriptorOwner(
                    descriptor.as_mut_ptr().cast(),
                    sid.as_ptr().cast_mut().cast(),
                    0,
                ) == 0
                || SetSecurityDescriptorDacl(descriptor.as_mut_ptr().cast(), 1, acl_ptr, 0) == 0
                || SetSecurityDescriptorControl(
                    descriptor.as_mut_ptr().cast(),
                    SE_DACL_PROTECTED,
                    SE_DACL_PROTECTED,
                ) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(Self {
                sid,
                acl,
                descriptor: descriptor.assume_init(),
            })
        }
    }
    pub fn as_ptr(&self) -> *mut std::ffi::c_void {
        (&self.descriptor as *const SECURITY_DESCRIPTOR)
            .cast_mut()
            .cast()
    }
    pub fn require_owner(&self, file: &File) -> Result<()> {
        let mut owner = null_mut();
        let mut descriptor = null_mut();
        // SAFETY: live handle, valid outputs; descriptor owns owner until LocalFree.
        let code = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                null_mut(),
                null_mut(),
                &mut descriptor,
            )
        };
        if code != 0 {
            return Err(std::io::Error::from_raw_os_error(code as i32).into());
        }
        let same = !owner.is_null()
            && unsafe { EqualSid(owner, self.sid.as_ptr().cast_mut().cast()) } != 0;
        unsafe {
            LocalFree(descriptor);
        }
        if same {
            Ok(())
        } else {
            Err(FileError::denied())
        }
    }
    /// Replaces the DACL of an existing object (app directory repair). New objects get this
    /// descriptor at creation instead, through `as_ptr`.
    pub fn apply(&self, file: &File) -> Result<()> {
        self.require_owner(file)?;
        let mut control = 0;
        let mut revision = 0;
        // SAFETY: this complete descriptor is owned by self and both outputs are live.
        if unsafe { GetSecurityDescriptorControl(self.as_ptr(), &mut control, &mut revision) } == 0
            || control & SE_DACL_PROTECTED == 0
        {
            return Err(FileError::denied());
        }
        // SAFETY: self owns the complete ACL and file is live. Owner is never changed here.
        let code = unsafe {
            SetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                self.acl.as_ptr().cast(),
                null_mut(),
            )
        };
        if code == 0 {
            Ok(())
        } else {
            Err(std::io::Error::from_raw_os_error(code as i32).into())
        }
    }
}

/// App directories live under the user profile, whose ACL grants other principals rights.
/// Owning the parent is enough there; the app directory handle itself is pinned.
pub(crate) fn trusted_app_parent(file: &File) -> Result<()> {
    trusted_parent(file)
        .or_else(|error| Security::new(true)?.require_owner(file).map_err(|_| error))
}

/// Owner is the current user, SYSTEM or Administrators, and no other principal has an
/// allow ACE that could add, remove or rename children or change the DACL.
pub(crate) fn trusted_parent(file: &File) -> Result<()> {
    let current = current_sid()?;
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

/// Protected DACL with exactly the owner's full-control ACE (inheritable for directories),
/// plus SYSTEM for app directories.
pub(crate) fn require_private_acl(file: &File, directory: bool) -> Result<()> {
    let mut owner = null_mut();
    let mut acl = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: all output storage is valid. Descriptor owns the returned SID and ACL.
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
        return Err(std::io::Error::from_raw_os_error(code as i32).into());
    }
    let result = (|| {
        if owner.is_null() || acl.is_null() {
            return Err(FileError::denied());
        }
        let mut control = 0;
        let mut revision = 0;
        let mut ace = null_mut();
        // SAFETY: descriptor, ACL and SID stay live through the final copy.
        unsafe {
            if GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) == 0
                || control & SE_DACL_PROTECTED == 0
                || !matches!((*acl).AceCount, 1 | 2)
                || GetAce(acl, 0, &mut ace) == 0
            {
                return Err(FileError::denied());
            }
            let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            if ace.Header.AceType != 0
                || ace.Header.AceFlags != if directory { 3 } else { 0 }
                || ace.Mask != 0x1f01ff
                || EqualSid(owner, (&ace.SidStart as *const u32).cast_mut().cast()) == 0
            {
                return Err(FileError::denied());
            }
            if (*acl).AceCount == 2 {
                let mut system = null_mut();
                let current = current_sid()?;
                if !directory
                    || EqualSid(owner, current.as_ptr().cast_mut().cast()) == 0
                    || GetAce(acl, 1, &mut system) == 0
                {
                    return Err(FileError::denied());
                }
                let system = &*system.cast::<ACCESS_ALLOWED_ACE>();
                if system.Header.AceType != 0
                    || system.Header.AceFlags != 0
                    || system.Mask != 0x1f01ff
                    || IsWellKnownSid(
                        (&system.SidStart as *const u32).cast_mut().cast(),
                        WinLocalSystemSid,
                    ) == 0
                {
                    return Err(FileError::denied());
                }
            }
            if EqualSid(owner, current_sid()?.as_ptr().cast_mut().cast()) == 0 {
                return Err(FileError::denied());
            }
            Ok(())
        }
    })();
    // SAFETY: GetSecurityInfo allocated descriptor and it is freed exactly once.
    unsafe {
        LocalFree(descriptor);
    }
    result
}
