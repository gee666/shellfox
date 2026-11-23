use super::{error, SystemResult};
use shellfox_protocol::ErrorCode;
use std::{ffi::OsString, path::PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemPlatform {
    Windows,
    Linux,
    Macos,
    Other,
}
pub trait SystemEnvironment: Send + Sync {
    fn platform(&self) -> SystemPlatform;
    fn home(&self) -> SystemResult<PathBuf>;
    fn variable(&self, name: &str) -> Option<OsString>;
}
pub struct NativeSystemEnvironment;
impl SystemEnvironment for NativeSystemEnvironment {
    fn platform(&self) -> SystemPlatform {
        if cfg!(windows) {
            SystemPlatform::Windows
        } else if cfg!(target_os = "linux") {
            SystemPlatform::Linux
        } else if cfg!(target_os = "macos") {
            SystemPlatform::Macos
        } else {
            SystemPlatform::Other
        }
    }
    fn variable(&self, name: &str) -> Option<OsString> {
        std::env::var_os(name)
    }
    fn home(&self) -> SystemResult<PathBuf> {
        #[cfg(windows)]
        {
            windows_home()
        }
        #[cfg(not(windows))]
        {
            let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
                error(
                    ErrorCode::NativeUnavailable,
                    "Host home directory is unavailable; no cwd/data-directory fallback was used.",
                )
            })?;
            super::validation::absolute_path(&home)?;
            Ok(home)
        }
    }
}
#[cfg(windows)]
fn windows_home() -> SystemResult<PathBuf> {
    use std::{ffi::c_void, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::TOKEN_QUERY,
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    #[link(name = "userenv")]
    extern "system" {
        fn GetUserProfileDirectoryW(token: *mut c_void, buffer: *mut u16, length: *mut u32) -> i32;
    }
    let mut token = std::ptr::null_mut();
    // SAFETY: querying the current process token does not change the account.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(error(
            ErrorCode::NativeUnavailable,
            "Host profile token is unavailable.",
        ));
    }
    struct Token(*mut c_void);
    impl Drop for Token {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    let token = Token(token);
    let mut length = 0;
    // SAFETY: bounded length query, then a correctly sized UTF-16 buffer.
    unsafe {
        GetUserProfileDirectoryW(token.0, std::ptr::null_mut(), &mut length);
    }
    if length == 0 || length > 32768 {
        return Err(error(
            ErrorCode::NativeUnavailable,
            "Host profile path length is invalid.",
        ));
    }
    let mut buffer = vec![0u16; length as usize];
    if unsafe { GetUserProfileDirectoryW(token.0, buffer.as_mut_ptr(), &mut length) } == 0 {
        return Err(error(
            ErrorCode::NativeUnavailable,
            "Host profile path cannot be queried.",
        ));
    }
    let end = buffer.iter().position(|value| *value == 0).ok_or_else(|| {
        error(
            ErrorCode::NativeUnavailable,
            "Host profile path is not terminated.",
        )
    })?;
    let value = PathBuf::from(OsString::from_wide(&buffer[..end]));
    super::validation::absolute_path(&value)?;
    Ok(value)
}
