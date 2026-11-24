//! A remote device never gets more than the host's own account. A root-run
//! host is that account, as in 0.9, so its ordinary tabs are shared like any
//! host's. Credentials that differ from the host account stay local-only.
use super::*;
use shellfox_runtime::{LaunchPrivilege, LaunchPrivilegeFacts};

pub(super) fn explicitly_elevated(facts: &LaunchPrivilegeFacts) -> bool {
    #[cfg(unix)]
    {
        elevated_for(facts, host_account())
    }
    #[cfg(windows)]
    {
        facts.privilege == LaunchPrivilege::Elevated
    }
}
/// `host` is the host's account, None when its real and effective IDs differ.
#[cfg(unix)]
fn elevated_for(facts: &LaunchPrivilegeFacts, host: Option<u32>) -> bool {
    let Some(owner) = host else {
        return true;
    };
    // A regular shell may offer sudo. Only its original credentials count.
    facts.privilege == LaunchPrivilege::Elevated
        || facts.posix.is_some_and(|launch| {
            [launch.uid, launch.euid, launch.saved_uid, launch.fs_uid]
                .iter()
                .any(|&uid| uid != owner)
        })
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

/// Said once when remote access starts, so a root host is a visible choice.
pub(super) fn note_root_remote_access() {
    #[cfg(unix)]
    if rustix::process::getuid().is_root() {
        eprintln!("Shellfox host is running as root; approved devices get root shells.");
    }
}

/// The host's account, None when its real and effective user IDs differ.
#[cfg(unix)]
pub(crate) fn host_account() -> Option<u32> {
    let user = rustix::process::getuid();
    (user == rustix::process::geteuid()).then_some(user.as_raw())
}
#[cfg(unix)]
fn normal_host_account() -> bool {
    host_account().is_some()
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use shellfox_runtime::PosixLaunchFacts;

    fn launched_as(uid: u32, euid: u32) -> LaunchPrivilegeFacts {
        LaunchPrivilegeFacts {
            privilege: LaunchPrivilege::Unknown,
            posix: cfg!(target_os = "linux").then_some(PosixLaunchFacts {
                uid,
                euid,
                saved_uid: euid,
                fs_uid: euid,
                effective_caps: 0,
                permitted_caps: 0,
                inheritable_caps: 0,
                ambient_caps: 0,
                no_new_privs: false,
            }),
            ..Default::default()
        }
    }
    #[test]
    fn a_root_host_shares_its_own_root_tabs() {
        assert!(!elevated_for(&launched_as(0, 0), Some(0)));
        assert!(!elevated_for(&LaunchPrivilegeFacts::default(), Some(0)));
    }
    #[test]
    fn credentials_other_than_the_host_account_stay_private() {
        let elevated = LaunchPrivilegeFacts {
            privilege: LaunchPrivilege::Elevated,
            ..Default::default()
        };
        for host in [0, 1000] {
            assert!(elevated_for(&elevated, Some(host)));
        }
        if cfg!(target_os = "linux") {
            assert!(elevated_for(&launched_as(1000, 1000), Some(0)));
            assert!(elevated_for(&launched_as(1000, 0), Some(1000)));
            assert!(!elevated_for(&launched_as(1000, 1000), Some(1000)));
        }
    }
    #[test]
    fn a_host_whose_real_and_effective_ids_differ_shares_nothing() {
        assert!(elevated_for(&launched_as(0, 0), None));
        assert!(elevated_for(&LaunchPrivilegeFacts::default(), None));
    }
}
