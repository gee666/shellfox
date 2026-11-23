//! One current-user SID / protected descriptor implementation for creation and legacy repair.
use crate::{FileError, Result};
use std::{fs::File, os::windows::io::AsRawHandle, ptr::null_mut};
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree},
    Security::{
        AddAccessAllowedAceEx,
        Authorization::{GetSecurityInfo, SetSecurityInfo, SE_FILE_OBJECT},
        CreateWellKnownSid, EqualSid, GetLengthSid, GetSecurityDescriptorControl,
        GetTokenInformation, InitializeAcl, InitializeSecurityDescriptor,
        SetSecurityDescriptorControl, SetSecurityDescriptorDacl, SetSecurityDescriptorOwner,
        TokenUser, WinLocalSystemSid, ACL, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
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
    /// Legacy explicit repair, NOT birth evidence. New creation uses as_ptr at the syscall.
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
