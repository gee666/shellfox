use super::*;
use shellfox_runtime::{LaunchPrivilege, LaunchPrivilegeFacts};

pub(super) fn explicitly_elevated(facts: &LaunchPrivilegeFacts) -> bool {
    if facts.privilege == LaunchPrivilege::Elevated {
        return true;
    }
    #[cfg(target_os = "linux")]
    if let Some(launch) = facts.posix {
        let owner = rustix::process::getuid().as_raw();
        // A regular shell may offer sudo. Only its original credentials count.
        return owner == 0
            || [launch.uid, launch.euid, launch.saved_uid, launch.fs_uid]
                .iter()
                .any(|&uid| uid != owner);
    }
    #[cfg(unix)]
    if rustix::process::geteuid().is_root() {
        return true;
    }
    false
}

pub(super) fn ensure_normal_launch() -> Result<(), AppError> {
    if normal_host_account() {
        Ok(())
    } else {
        Err(failure(
            ErrorCode::AccessDenied,
            "Remote terminals cannot inherit an elevated host account.",
        ))
    }
}

#[cfg(unix)]
fn normal_host_account() -> bool {
    let user = rustix::process::getuid();
    !user.is_root() && user == rustix::process::geteuid()
}
#[cfg(windows)]
fn normal_host_account() -> bool {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let mut token = std::ptr::null_mut();
    // SAFETY: current process pseudo-handle and writable slot for its token.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return false;
    }
    let mut elevation = TOKEN_ELEVATION::default();
    let mut bytes = 0;
    // SAFETY: the token is owned and the buffer has the exact Win32 layout.
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            std::ptr::addr_of_mut!(elevation).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut bytes,
        )
    };
    // SAFETY: the handle was allocated by OpenProcessToken and is closed once.
    unsafe {
        CloseHandle(token);
    }
    ok != 0 && bytes as usize == size_of::<TOKEN_ELEVATION>() && elevation.TokenIsElevated == 0
}
